use evetools_catalog::{CatalogConfig, CatalogService, CatalogServiceError, CatalogStatus};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("failed to import SDE catalog: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), CatalogServiceError> {
    let config = CatalogConfig::from_env()?;
    let service = CatalogService::connect(config).await?;
    let status = service.import_latest().await?;
    print_status(&status);
    Ok(())
}

fn print_status(status: &CatalogStatus) {
    println!("SDE catalog import status:");
    println!("  status: {}", status.status);
    println!("  build_number: {}", display_i32(status.build_number));
    println!(
        "  source_url: {}",
        display_str(status.source_url.as_deref())
    );
    println!(
        "  completed_at: {}",
        display_str(status.completed_at.as_deref())
    );
    println!("  type_count: {}", status.type_count);
    println!("  group_count: {}", status.group_count);
    println!("  category_count: {}", status.category_count);
    println!("  market_group_count: {}", status.market_group_count);
    if let Some(error_summary) = status.error_summary.as_deref() {
        println!("  error_summary: {error_summary}");
    }
}

fn display_i32(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn display_str(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}
