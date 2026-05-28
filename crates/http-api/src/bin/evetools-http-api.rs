#[tokio::main]
async fn main() {
    if let Err(error) = evetools_http_api::serve_from_env().await {
        eprintln!("failed to run EveTools HTTP API: {error}");
        std::process::exit(1);
    }
}
