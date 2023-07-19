use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use arboard::{Clipboard, ImageData};
use futures_channel::mpsc::UnboundedSender;
use futures_util::{future::select, pin_mut, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::spawn;
use tokio_tungstenite::connect_async_with_config;
use tungstenite::Message;
use ulid::Ulid;

use crate::config::WEB_SOCKET_CONFIG;

enum ClipboardCache<'a> {
    Text(String),
    Image(ImageData<'a>),
}

struct ClientState {
    cache: ClipboardCache<'static>,
    image_info: ClipboardMessageImage,
    id: String,
    timestamp: u64,
}

#[derive(Serialize, Deserialize)]
struct ClipboardMessageImage {
    width: usize,
    height: usize,
}

#[derive(Serialize, Deserialize)]
struct ClipboardMessageText {
    content: String,
}

#[derive(Serialize, Deserialize)]
enum ClipboardMessagePayload {
    Text(ClipboardMessageText),
    Image(ClipboardMessageImage),
}

#[derive(Serialize, Deserialize)]
struct ClipboardMessage {
    payload: ClipboardMessagePayload,
}

fn serialize_clipboard_message(payload: ClipboardMessagePayload) -> String {
    let message = ClipboardMessage { payload };

    serde_json::to_string(&message).unwrap()
}

async fn check_clipboard(sender: UnboundedSender<Message>, state: Arc<Mutex<ClientState>>) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let mut state = state.lock().unwrap();
        let mut clipboard = Clipboard::new().unwrap();
        let current = clipboard.get_image();
        match current {
            Ok(current) => {
                if let ClipboardCache::Image(image) = &state.cache {
                    if image.bytes != current.bytes {
                        let payload = serialize_clipboard_message(ClipboardMessagePayload::Image(
                            ClipboardMessageImage {
                                width: image.width,
                                height: image.height,
                            },
                        ));
                        sender.unbounded_send(Message::Text(payload)).unwrap();
                        sender
                            .unbounded_send(Message::Binary(current.bytes.to_vec()))
                            .unwrap();
                        state.cache = ClipboardCache::Image(current);
                    }
                } else {
                    let payload = serialize_clipboard_message(ClipboardMessagePayload::Image(
                        ClipboardMessageImage {
                            width: current.width,
                            height: current.height,
                        },
                    ));
                    sender.unbounded_send(Message::Text(payload)).unwrap();
                    sender
                        .unbounded_send(Message::Binary(current.bytes.to_vec()))
                        .unwrap();
                    state.cache = ClipboardCache::Image(current);
                }
            }
            Err(arboard::Error::ContentNotAvailable) => {
                let current = clipboard.get_text();
                match current {
                    Ok(current) => {
                        if let ClipboardCache::Text(text) = &state.cache {
                            if text != &current {
                                let payload = serialize_clipboard_message(
                                    ClipboardMessagePayload::Text(ClipboardMessageText {
                                        content: current.to_string(),
                                    }),
                                );
                                sender.unbounded_send(Message::Text(payload)).unwrap();
                                state.cache = ClipboardCache::Text(current);
                            }
                        } else {
                            let payload = serialize_clipboard_message(
                                ClipboardMessagePayload::Text(ClipboardMessageText {
                                    content: current.to_string(),
                                }),
                            );
                            sender.unbounded_send(Message::Text(payload)).unwrap();
                            state.cache = ClipboardCache::Text(current);
                        }
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }
    }
}

fn generate_ulid() -> String {
    let ulid = Ulid::new();
    ulid.to_string()
}

fn handle_text_message(clipboard_message: String, state: Arc<Mutex<ClientState>>) {
    let mut state = state.lock().unwrap();

    let deserialized: ClipboardMessage = serde_json::from_str(&clipboard_message).unwrap();
    match deserialized.payload {
        ClipboardMessagePayload::Text(payload) => {
            let mut clipboard = Clipboard::new().unwrap();
            clipboard.set_text(payload.content.to_string()).unwrap();
            state.cache = ClipboardCache::Text(payload.content);
        }
        ClipboardMessagePayload::Image(payload) => {
            state.image_info = payload;
        }
    }
}

pub async fn connect(addr: String) {
    let state = Arc::new(Mutex::new(ClientState {
        cache: ClipboardCache::Text(String::new()),
        image_info: ClipboardMessageImage {
            width: 0,
            height: 0,
        },
        id: generate_ulid(),
        timestamp: 0,
    }));

    let (tx, rx) = futures_channel::mpsc::unbounded();

    let (ws, _) = connect_async_with_config(addr, Some(WEB_SOCKET_CONFIG))
        .await
        .expect("Failed to connect");

    let (write, read) = ws.split();

    let forward_ws = rx.map(Ok).forward(write);

    let handler = {
        read.for_each(|message| async {
            let message = message.unwrap();
            match message {
                Message::Text(text) => {
                    handle_text_message(text, state.clone());
                }
                Message::Binary(binary) => {
                    let mut client_state = state.lock().unwrap();
                    let mut clipboard = Clipboard::new().unwrap();

                    let image = ImageData {
                        width: client_state.image_info.width,
                        height: client_state.image_info.height,
                        bytes: Cow::from(binary),
                    };
                    clipboard.set_image(image.clone()).unwrap();
                    client_state.cache = ClipboardCache::Image(image);
                }
                _ => {
                    println!("unknow, {}", message);
                }
            }
        })
    };

    spawn(check_clipboard(tx, state.clone()));

    pin_mut!(forward_ws, handler);

    select(forward_ws, handler).await;
}
