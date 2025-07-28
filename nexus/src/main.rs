use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use args::Args;
use clap::Parser;
use config::Config;
use server::ServeConfig;

mod args;
mod logger;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = args.config();

    logger::init(&args);
    server::serve(serve_config(&args, config)).await?;

    Ok(())
}

fn serve_config(args: &Args, config: Config) -> ServeConfig {
    let listen_address = args
        .listen_address
        .or(config.server.listen_address)
        .unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8000)));

    ServeConfig { listen_address, config }
}
