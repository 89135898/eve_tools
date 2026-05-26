use serde::Deserialize;
use thiserror::Error;
use url::Url;

const LATEST_METADATA_PATH: &str = "/static-data/tranquility/latest.jsonl";
const LATEST_ARCHIVE_PATH: &str = "/static-data/eve-online-static-data-latest-jsonl.zip";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdeLatestMetadata {
    pub build_number: i32,
    pub release_date: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLatestMetadata {
    #[serde(rename = "_key")]
    _key: String,
    build_number: i32,
    release_date: String,
}

#[derive(Debug, Error)]
pub enum SdeClientError {
    #[error("invalid SDE URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("SDE request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SDE response decode failed: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("SDE latest metadata response was empty")]
    EmptyMetadata,
}

#[derive(Clone, Debug)]
pub struct SdeClient {
    base_url: Url,
    http: reqwest::Client,
}

impl SdeClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, SdeClientError> {
        Ok(Self {
            base_url: Url::parse(base_url.as_ref())?,
            http: reqwest::Client::builder()
                .user_agent("EveTools SDE importer")
                .build()?,
        })
    }

    pub fn official() -> Result<Self, SdeClientError> {
        Self::new("https://developers.eveonline.com")
    }

    pub async fn latest_metadata(&self) -> Result<SdeLatestMetadata, SdeClientError> {
        let body = self
            .http
            .get(self.base_url.join(LATEST_METADATA_PATH)?)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let line = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .ok_or(SdeClientError::EmptyMetadata)?;
        let raw: RawLatestMetadata = serde_json::from_str(line)?;
        Ok(SdeLatestMetadata {
            build_number: raw.build_number,
            release_date: raw.release_date,
        })
    }

    pub async fn download_latest_archive(&self) -> Result<Vec<u8>, SdeClientError> {
        Ok(self
            .http
            .get(self.base_url.join(LATEST_ARCHIVE_PATH)?)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }
}
