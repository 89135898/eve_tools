use evetools_catalog::{
    CatalogConfig, CatalogImportProgress, CatalogService, CatalogServiceError, CatalogStatus,
};

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
    let status = service
        .import_latest_with_progress(|progress| println!("{}", format_progress(&progress)))
        .await?;
    print_status(&status);
    Ok(())
}

fn format_progress(progress: &CatalogImportProgress) -> String {
    match progress {
        CatalogImportProgress::CheckingLatestMetadata => {
            "[1/5] checking latest SDE metadata...".to_string()
        }
        CatalogImportProgress::CheckingCurrentCatalog {
            latest_build_number,
        } => {
            format!("[2/5] checking current catalog status for build {latest_build_number}...")
        }
        CatalogImportProgress::AlreadyCurrent { build_number } => {
            format!("[2/5] catalog already current for build {build_number}")
        }
        CatalogImportProgress::DownloadingArchive { .. } => {
            "[3/5] downloading archive...".to_string()
        }
        CatalogImportProgress::DownloadedArchive { byte_count } => {
            format!("[3/5] downloaded archive: {}", format_mib(*byte_count))
        }
        CatalogImportProgress::ParsingArchive => "[4/5] parsing archive...".to_string(),
        CatalogImportProgress::ParsedArchive {
            type_count,
            group_count,
            category_count,
            market_group_count,
        } => format!(
            "      parsed: types={type_count}, groups={group_count}, categories={category_count}, market groups={market_group_count}"
        ),
        CatalogImportProgress::WritingCatalog => "[5/5] writing to Postgres...".to_string(),
        CatalogImportProgress::WritingTableStarted { table, total } => {
            format!("      {}: 0 / {total}", table.as_str())
        }
        CatalogImportProgress::WritingRows {
            table,
            completed,
            total,
        } => {
            format!("      {}: {completed} / {total}", table.as_str())
        }
        CatalogImportProgress::DeletingStaleRows => {
            "      removing stale catalog rows...".to_string()
        }
        CatalogImportProgress::Completed { status } => {
            format!(
                "done: {}, build={}",
                status.status,
                display_i32(status.build_number)
            )
        }
    }
}

fn format_mib(byte_count: usize) -> String {
    format!("{:.1} MiB", byte_count as f64 / 1_048_576.0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use evetools_catalog::CatalogImportTable;

    #[test]
    fn formats_row_progress() {
        let progress = CatalogImportProgress::WritingRows {
            table: CatalogImportTable::Types,
            completed: 2_000,
            total: 45_000,
        };

        assert_eq!(format_progress(&progress), "      types: 2000 / 45000");
    }

    #[test]
    fn formats_download_completion_with_size() {
        let progress = CatalogImportProgress::DownloadedArchive {
            byte_count: 104_857_600,
        };

        assert_eq!(
            format_progress(&progress),
            "[3/5] downloaded archive: 100.0 MiB"
        );
    }
}
