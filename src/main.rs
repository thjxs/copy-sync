use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
};

use arboard::Clipboard;
use clap::{Parser, Subcommand};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::connect_async;
use tungstenite::Message;

type Tx = UnboundedSender<Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

struct ClipboardState {
    cache: String,
    instance: Clipboard,
    count: i32,
}

async fn handle_connection(peer_map: PeerMap, raw_stream: TcpStream, addr: SocketAddr) {
    let ws = tokio_tungstenite::accept_async(raw_stream)
        .await
        .expect("whoops");

    let (tx, rx) = unbounded();

    peer_map.lock().unwrap().insert(addr, tx);

    let (outgoing, incoming) = ws.split();

    let broadcast_incoming = incoming.try_for_each(|msg| {
        let peers = peer_map.lock().unwrap();

        let broadcast_recipients = peers
            .iter()
            .filter(|(peer_addr, _)| peer_addr != &&addr)
            .map(|(_, ws_sink)| ws_sink);

        for rec in broadcast_recipients {
            rec.unbounded_send(msg.clone()).unwrap();
        }

        future::ok(())
    });

    let receive_from_others = rx.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    peer_map.lock().unwrap().remove(&addr);
}

async fn create_server(port: u16) -> Result<(), io::Error> {
    let addr = format!("0.0.0.0:{}", port);
    let server = TcpListener::bind(addr).await;
    let state = PeerMap::new(Mutex::new(HashMap::new()));
    let listener = server.expect("Failed to create server");

    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(handle_connection(state.clone(), stream, addr));
    }

    Ok(())
}

async fn check_clipboard(
    sender: futures_channel::mpsc::UnboundedSender<Message>,
    state: Arc<Mutex<ClipboardState>>,
) {
    println!("Checking clipboard");
    loop {
        sleep(Duration::from_secs(2));
        let mut state = state.lock().unwrap();
        let current = state.instance.get_text();
        let current = match current {
            Ok(current) => current,
            Err(_) => continue,
        };
        if current != state.cache {
            state.cache = current;
            sender
                .unbounded_send(Message::Text(state.cache.clone()))
                .unwrap();
        }
    }
}

async fn create_client(addr: String) {
    let state = ClipboardState {
        cache: String::new(),
        instance: Clipboard::new().unwrap(),
        count: 0,
    };
    let state = Arc::new(Mutex::new(state));
    let (tx, rx) = futures_channel::mpsc::unbounded();

    let (ws, _) = connect_async(addr).await.expect("Failed to connect");
    let (write, read) = ws.split();

    let rx_to_ws = rx.map(Ok).forward(write);

    let ws_to_clipboard = {
        read.for_each(|message| async {
            let mut state = state.lock().unwrap();
            state.cache = message.unwrap().to_string();
            let text = state.cache.to_string();
            state.instance.set_text(text).unwrap();
            state.count += 1;
            println!("sync remote clipboard: {}", state.count);
        })
    };

    tokio::spawn(check_clipboard(tx, state.clone()));

    pin_mut!(rx_to_ws, ws_to_clipboard);

    future::select(rx_to_ws, ws_to_clipboard).await;
}

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    CreateServer {
        #[arg(short, long, default_value_t = 5120)]
        port: u16,
    },
    CreateClient {
        #[arg(short, long)]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::CreateServer { port }) => {
            create_server(port).await.unwrap();
        }
        Some(Commands::CreateClient { addr }) => {
            create_client(addr).await;
        }
        None => {}
    }

    Ok(())
}
