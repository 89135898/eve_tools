use evetools_worker::{
    format_public_market_sync_summary, run_public_market_region_sync, PublicMarketSyncCliConfig,
};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("failed to sync public market region: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), evetools_worker::PublicMarketSyncCliError> {
    let config = PublicMarketSyncCliConfig::from_env_and_args(std::env::args().skip(1))?;
    let summary = run_public_market_region_sync(config).await?;
    println!("{}", format_public_market_sync_summary(&summary));
    Ok(())
}
