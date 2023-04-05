use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use futures_channel::mpsc::UnboundedSender;
use futures_util::{StreamExt, TryStreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::accept_async;
use tungstenite::Message;

type UnboundedMessage = UnboundedSender<Message>;

type PeerMap = Arc<Mutex<HashMap<SocketAddr, UnboundedMessage>>>;

pub async fn handle_connection(map: PeerMap, raw_stream: TcpStream, addr: SocketAddr) {
    let ws = accept_async(raw_stream).await.expect("whoops");

    let (tx, rx) = futures_channel::mpsc::unbounded();

    map.lock().unwrap().insert(addr, tx);

    let (outgoing, incoming) = ws.split();

    let broadcast_incoming = incoming.try_for_each(|msg| {
        match msg {
            Message::Close(_) => {}
            _ => {
                let peers = map.lock().unwrap();

                let broadcast_recipients = peers
                    .iter()
                    .filter(|(peer_addr, _)| peer_addr != &&addr)
                    .map(|(_, ws_sink)| ws_sink);

                for rec in broadcast_recipients {
                    rec.unbounded_send(msg.clone()).unwrap();
                }
            }
        }

        futures_util::future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    futures_util::pin_mut!(broadcast_incoming, receive_from_others);
    futures_util::future::select(broadcast_incoming, receive_from_others).await;

    map.lock().unwrap().remove(&addr);
}

pub async fn start(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let state = PeerMap::new(Mutex::new(HashMap::new()));
    let server = tokio::net::TcpListener::bind(addr).await;
    let listener = server.expect("Failed to create server");

    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(handle_connection(state.clone(), stream, addr));
    }
}
