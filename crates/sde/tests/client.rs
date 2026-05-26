use evetools_sde::{SdeClient, SdeClientError};
use httpmock::prelude::*;
use std::time::Duration;

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

#[tokio::test]
async fn metadata_http_status_error_is_http_error() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/static-data/tranquility/latest.jsonl");
            then.status(503).body("unavailable");
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let error = client.latest_metadata().await.unwrap_err();

    mock.assert_async().await;
    assert!(matches!(error, SdeClientError::Http(_)));
}

#[tokio::test]
async fn metadata_invalid_json_is_decode_error() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/static-data/tranquility/latest.jsonl");
            then.status(200).body("not-json");
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let error = client.latest_metadata().await.unwrap_err();

    mock.assert_async().await;
    assert!(matches!(error, SdeClientError::Decode(_)));
}

#[tokio::test]
async fn metadata_empty_body_is_empty_metadata_error() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/static-data/tranquility/latest.jsonl");
            then.status(200).body("\n \t\n");
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let error = client.latest_metadata().await.unwrap_err();

    mock.assert_async().await;
    assert!(matches!(error, SdeClientError::EmptyMetadata));
}

#[test]
fn constructs_with_custom_request_timeout() {
    let client =
        SdeClient::with_request_timeout("https://developers.eveonline.com", Duration::from_secs(1));

    assert!(client.is_ok());
}
