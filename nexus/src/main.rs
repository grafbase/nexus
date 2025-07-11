use args::Args;
use clap::Parser;

mod args;
mod logger;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    logger::init(&args);

    {
        log::error!("Hello error!");
        log::warn!("Hello warn!");
        log::info!(foo = "bar"; "Hello info!");
        log::debug!("Hello debug!");
        log::trace!("Hello trace!");
    }

    fastrace::flush();

    Ok(())
}
