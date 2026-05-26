use evetools_sde::SdeClient;
use httpmock::prelude::*;

#[tokio::test]
async fn fetches_latest_metadata() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/static-data/tranquility/latest.jsonl");
            then.status(200).body(
                r#"{"_key":"sde","buildNumber":3351823,"releaseDate":"2026-05-19T12:12:31Z"}"#,
            );
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let metadata = client.latest_metadata().await.unwrap();

    mock.assert_async().await;
    assert_eq!(metadata.build_number, 3_351_823);
}

#[tokio::test]
async fn downloads_latest_archive_bytes() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/static-data/eve-online-static-data-latest-jsonl.zip");
            then.status(200).body("zip-bytes");
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let bytes = client.download_latest_archive().await.unwrap();

    mock.assert_async().await;
    assert_eq!(bytes.as_slice(), b"zip-bytes");
}
