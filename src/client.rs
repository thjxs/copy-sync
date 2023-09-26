use arboard::{Clipboard, ImageData};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use futures_channel::mpsc::UnboundedSender;
use futures_util::{future::select, pin_mut, StreamExt};
use serde::{Deserialize, Serialize};
use std::io::prelude::*;
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};
use tokio::net::TcpStream;
use tokio::spawn;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};
use tungstenite::Message;
use ulid::Ulid;

use crate::config::{RETRY_CONNECT_INTERVAL_IN_SECONDS, WEB_SOCKET_CONFIG};
use crate::notify::notify;

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

fn encode(bytes: Vec<u8>) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&bytes[..]).unwrap();
    encoder.finish().unwrap()
}

fn decode(bytes: Vec<u8>) -> Vec<u8> {
    let mut decoder = ZlibDecoder::new(&bytes[..]);
    let mut decoded_bytes = Vec::new();
    decoder.read_to_end(&mut decoded_bytes).unwrap();
    decoded_bytes
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
                    if image.bytes == current.bytes {
                        continue;
                    }
                }
                let payload = serialize_clipboard_message(ClipboardMessagePayload::Image(
                    ClipboardMessageImage {
                        width: current.width,
                        height: current.height,
                    },
                ));
                sender.unbounded_send(Message::Text(payload)).unwrap();
                // compress image
                sender
                    .unbounded_send(Message::Binary(encode(current.bytes.to_vec())))
                    .unwrap();
                state.cache = ClipboardCache::Image(current);
            }
            Err(arboard::Error::ContentNotAvailable) => {
                let current = clipboard.get_text();
                match current {
                    Ok(current) => {
                        if let ClipboardCache::Text(text) = &state.cache {
                            if text == &current {
                                continue;
                            }
                        }
                        let payload = serialize_clipboard_message(ClipboardMessagePayload::Text(
                            ClipboardMessageText {
                                content: current.to_string(),
                            },
                        ));
                        sender.unbounded_send(Message::Text(payload)).unwrap();
                        state.cache = ClipboardCache::Text(current);
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

fn handle_message(message: Message, state: Arc<Mutex<ClientState>>) {
    let mut state = state.lock().unwrap();
    match message {
        Message::Text(text) => {
            let deserialized: ClipboardMessage = serde_json::from_str(&text).unwrap();
            match deserialized.payload {
                ClipboardMessagePayload::Text(payload) => {
                    let mut clipboard = Clipboard::new().unwrap();
                    let result = clipboard.set_text(&payload.content);
                    if result.is_err() {
                        println!("set text error: {:?}", result);
                    }
                    state.cache = ClipboardCache::Text(payload.content);
                    state.id = generate_ulid();
                    state.timestamp = 0;
                }
                ClipboardMessagePayload::Image(payload) => {
                    state.image_info = payload;
                    state.id = generate_ulid();
                    state.timestamp = 0;
                }
            }
        }
        Message::Binary(binary) => {
            let mut clipboard = Clipboard::new().unwrap();
            let bytes = decode(binary);

            let image = ImageData {
                width: state.image_info.width,
                height: state.image_info.height,
                bytes: Cow::from(bytes),
            };
            let result = clipboard.set_image(image.clone());
            if result.is_err() {
                println!("set image error: {:?}", result);
            }
            state.cache = ClipboardCache::Image(image);
            let info = &state.image_info;
            notify(&format!("W: {} H: {}", info.width, info.height));
        }
        _ => {
            println!("unknow, {}", message);
        }
    }
}

async fn run(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) {
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

    let (write, read) = ws.split();

    let forward_ws = rx.map(Ok).forward(write);

    let handler = {
        read.for_each(|message| async {
            match message {
                Ok(message) => {
                    handle_message(message, state.clone());
                }
                Err(err) => {
                    println!("{:?}", err);
                }
            }
        })
    };

    let check_clipboard_handler = spawn(check_clipboard(tx, state.clone()));

    pin_mut!(forward_ws, handler);

    select(forward_ws, handler).await;

    check_clipboard_handler.abort();
}

pub async fn start(addr: String) {
    loop {
        let result = connect_async_with_config(&addr, Some(WEB_SOCKET_CONFIG)).await;
        match result {
            Ok((ws, _)) => {
                println!("Connected: {}", addr);
                run(ws).await;
            }
            Err(err) => {
                println!("{:?}", err);
                tokio::time::sleep(tokio::time::Duration::from_secs(
                    RETRY_CONNECT_INTERVAL_IN_SECONDS,
                ))
                .await;
                println!("Reconnecting: {}...", addr);
            }
        }
    }
}
