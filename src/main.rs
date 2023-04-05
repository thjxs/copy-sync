use std::io;

pub mod client;
pub mod server;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long, default_value_t = 5120)]
        port: u16,
    },
    Connect {
        #[arg(short, long)]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Start { port }) => server::start(port).await,
        Some(Commands::Connect { addr }) => {
            client::connect(addr).await;
        }
        None => {}
    }

    Ok(())
}
