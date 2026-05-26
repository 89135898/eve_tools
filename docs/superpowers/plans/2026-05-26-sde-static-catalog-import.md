# SDE Static Catalog Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust catalog service that imports official EVE SDE JSONL data into Supabase Postgres and exposes localized item lookup/search through Tauri commands.

**Architecture:** `crates/sde` owns SDE download, zip reading, and JSONL parsing. `crates/db` owns Postgres migrations and catalog repository queries. `crates/catalog` is the application service boundary used by Tauri commands; React only calls Tauri commands and never reads SDE files or database URLs.

**Tech Stack:** Rust 1.82, Tauri 2, `sqlx` Postgres runtime, Supabase Postgres through `EVETOOLS_DATABASE_URL`, `zip`, `serde`, `serde_json`, `reqwest`, `tokio`, `tempfile`, React/Vite TypeScript wrappers.

---

## Safety Rules

- Do not commit database URLs, passwords, Supabase project refs, or `.env` files.
- Use `EVETOOLS_DATABASE_URL` for runtime database access.
- Use `EVETOOLS_TEST_DATABASE_URL` for integration tests that touch Postgres.
- Prefer the Supabase direct Postgres URL with `sslmode=require` for migration/import work.
- If `EVETOOLS_TEST_DATABASE_URL` is missing, Postgres integration tests must skip themselves rather than fail.
- Tauri commands must call `CatalogService`; they must not construct SQL directly.

## File Structure

- Modify `Cargo.toml`: add `crates/sde`, `crates/catalog`, and workspace dependencies.
- Create `crates/sde`: SDE parser, archive reader, official download client.
- Modify `crates/db`: Postgres migration and repository.
- Create `crates/catalog`: service config and orchestration.
- Modify `apps/desktop/src-tauri`: catalog commands that call `CatalogService`.
- Modify `apps/desktop/src/commands.ts`: typed command wrappers.
- Modify `README.md`: Supabase catalog setup and secret handling.

## Task 1: Workspace Dependencies And Crates

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/sde/Cargo.toml`
- Create: `crates/sde/src/lib.rs`
- Create: `crates/catalog/Cargo.toml`
- Create: `crates/catalog/src/lib.rs`

- [ ] **Step 1: Add workspace members and dependencies**

Modify root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
  "crates/domain",
  "crates/esi",
  "crates/db",
  "crates/sde",
  "crates/catalog",
  "crates/worker",
  "apps/desktop/src-tauri"
]

[workspace.package]
edition = "2021"
license = "UNLICENSED"
version = "0.1.0"
rust-version = "1.82"

[workspace.dependencies]
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls"] }
rust_decimal = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio-rustls", "postgres", "chrono", "json"] }
tauri = { version = "2" }
tauri-build = { version = "2" }
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
url = "2"
zip = { version = "5", default-features = false, features = ["deflate"] }
```

- [ ] **Step 2: Create SDE crate**

Create `crates/sde/Cargo.toml`:

```toml
[package]
name = "evetools-sde"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
url.workspace = true
zip.workspace = true

[dev-dependencies]
httpmock = "0.8.3"
tempfile.workspace = true
tokio.workspace = true
```

Create `crates/sde/src/lib.rs`:

```rust
pub fn sde_crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sde_crate_reports_ready() {
        assert!(sde_crate_ready());
    }
}
```

- [ ] **Step 3: Create catalog service crate**

Create `crates/catalog/Cargo.toml`:

```toml
[package]
name = "evetools-catalog"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
evetools-db = { path = "../db" }
evetools-sde = { path = "../sde" }
serde.workspace = true
thiserror.workspace = true
tokio.workspace = true
```

Create `crates/catalog/src/lib.rs`:

```rust
pub fn catalog_crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_crate_reports_ready() {
        assert!(catalog_crate_ready());
    }
}
```

- [ ] **Step 4: Verify skeleton**

Run:

```bash
cargo test -p evetools-sde -p evetools-catalog
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/sde crates/catalog Cargo.lock
git commit -m "feat: add sde catalog service crates"
```

## Task 2: SDE Parser And Archive Reader

**Files:**

- Create: `crates/sde/src/models.rs`
- Create: `crates/sde/src/parser.rs`
- Create: `crates/sde/src/archive.rs`
- Replace: `crates/sde/src/lib.rs`
- Create: `crates/sde/tests/parser.rs`

- [ ] **Step 1: Write parser/archive tests**

Create `crates/sde/tests/parser.rs`:

```rust
use evetools_sde::{
    parse_type_line, read_catalog_archive_from_bytes, CatalogArchive, CatalogType,
};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn test_zip() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("_sde.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":"sde","buildNumber":3351823,"releaseDate":"2026-05-19T12:12:31Z"}}"#
    )
    .unwrap();
    zip.start_file("types.jsonl", options).unwrap();
    writeln!(zip, r#"{{"_key":34,"description":{{"en":"Primary building block","zh":"主要建造材料"}},"groupID":18,"marketGroupID":1857,"name":{{"en":"Tritanium","zh":"三钛合金"}},"packagedVolume":0.01,"portionSize":1,"published":true,"volume":0.01}}"#).unwrap();
    zip.start_file("groups.jsonl", options).unwrap();
    writeln!(zip, r#"{{"_key":18,"categoryID":4,"name":{{"en":"Mineral","zh":"矿物"}},"published":true}}"#).unwrap();
    zip.start_file("categories.jsonl", options).unwrap();
    writeln!(zip, r#"{{"_key":4,"name":{{"en":"Material","zh":"材料"}},"published":true}}"#).unwrap();
    zip.start_file("marketGroups.jsonl", options).unwrap();
    writeln!(zip, r#"{{"_key":1857,"description":{{"en":"Raw materials"}},"name":{{"en":"Minerals","zh":"矿物"}},"parentGroupID":1031}}"#).unwrap();

    zip.finish().unwrap().into_inner()
}

#[test]
fn parses_type_line_with_localized_names() {
    let row: CatalogType = parse_type_line(
        r#"{"_key":34,"description":{"en":"Primary building block","zh":"主要建造材料"},"groupID":18,"marketGroupID":1857,"name":{"en":"Tritanium","zh":"三钛合金"},"published":true,"volume":0.01}"#,
    )
    .unwrap();

    assert_eq!(row.type_id, 34);
    assert_eq!(row.group_id, 18);
    assert_eq!(row.market_group_id, Some(1857));
    assert_eq!(row.name_en.as_deref(), Some("Tritanium"));
    assert_eq!(row.name_zh.as_deref(), Some("三钛合金"));
}

#[test]
fn reads_required_catalog_files_from_zip_bytes() {
    let archive: CatalogArchive = read_catalog_archive_from_bytes(test_zip()).unwrap();

    assert_eq!(archive.metadata.build_number, Some(3_351_823));
    assert_eq!(archive.types.len(), 1);
    assert_eq!(archive.groups.len(), 1);
    assert_eq!(archive.categories.len(), 1);
    assert_eq!(archive.market_groups.len(), 1);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p evetools-sde --test parser
```

Expected: FAIL with unresolved parser/archive symbols.

- [ ] **Step 3: Implement SDE models**

Create `crates/sde/src/models.rs`:

```rust
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogType {
    pub type_id: i32,
    pub group_id: i32,
    pub market_group_id: Option<i32>,
    pub published: bool,
    pub volume: Option<f64>,
    pub packaged_volume: Option<f64>,
    pub capacity: Option<f64>,
    pub mass: Option<f64>,
    pub portion_size: Option<i32>,
    pub meta_level: Option<i32>,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub description_en: Option<String>,
    pub description_zh: Option<String>,
    pub raw_name_json: serde_json::Value,
    pub raw_description_json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogGroup {
    pub group_id: i32,
    pub category_id: i32,
    pub published: bool,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub raw_name_json: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogCategory {
    pub category_id: i32,
    pub published: bool,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub raw_name_json: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogMarketGroup {
    pub market_group_id: i32,
    pub parent_group_id: Option<i32>,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub description_en: Option<String>,
    pub description_zh: Option<String>,
    pub raw_name_json: serde_json::Value,
    pub raw_description_json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdeMetadata {
    pub build_number: Option<i32>,
    pub release_date: Option<String>,
}

pub(crate) type LocalizedMap = BTreeMap<String, String>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawTypeRow {
    #[serde(rename = "_key")]
    pub key: i32,
    pub group_id: i32,
    pub market_group_id: Option<i32>,
    #[serde(default)]
    pub published: bool,
    pub volume: Option<f64>,
    pub packaged_volume: Option<f64>,
    pub capacity: Option<f64>,
    pub mass: Option<f64>,
    pub portion_size: Option<i32>,
    pub meta_level: Option<i32>,
    #[serde(default)]
    pub name: LocalizedMap,
    #[serde(default)]
    pub description: LocalizedMap,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawGroupRow {
    #[serde(rename = "_key")]
    pub key: i32,
    pub category_id: i32,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub name: LocalizedMap,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawCategoryRow {
    #[serde(rename = "_key")]
    pub key: i32,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub name: LocalizedMap,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawMarketGroupRow {
    #[serde(rename = "_key")]
    pub key: i32,
    pub parent_group_id: Option<i32>,
    #[serde(default)]
    pub name: LocalizedMap,
    #[serde(default)]
    pub description: LocalizedMap,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSdeMetadataRow {
    #[serde(rename = "_key")]
    pub _key: String,
    pub build_number: Option<i32>,
    pub release_date: Option<String>,
}
```

- [ ] **Step 4: Implement parser**

Create `crates/sde/src/parser.rs`:

```rust
use crate::models::*;
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdeParseError {
    #[error("failed to parse {row_kind} row: {source}")]
    Json {
        row_kind: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

fn parse_row<T: DeserializeOwned>(
    row_kind: &'static str,
    line: &str,
) -> Result<T, SdeParseError> {
    serde_json::from_str(line).map_err(|source| SdeParseError::Json { row_kind, source })
}

fn localized(map: &LocalizedMap, key: &str) -> Option<String> {
    map.get(key).filter(|value| !value.is_empty()).cloned()
}

fn raw_json(map: &LocalizedMap) -> serde_json::Value {
    serde_json::to_value(map).expect("localized map should serialize")
}

fn optional_raw_json(map: &LocalizedMap) -> Option<serde_json::Value> {
    if map.is_empty() {
        None
    } else {
        Some(raw_json(map))
    }
}

pub fn parse_type_line(line: &str) -> Result<CatalogType, SdeParseError> {
    let raw: RawTypeRow = parse_row("types", line)?;
    Ok(CatalogType {
        type_id: raw.key,
        group_id: raw.group_id,
        market_group_id: raw.market_group_id,
        published: raw.published,
        volume: raw.volume,
        packaged_volume: raw.packaged_volume,
        capacity: raw.capacity,
        mass: raw.mass,
        portion_size: raw.portion_size,
        meta_level: raw.meta_level,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        description_en: localized(&raw.description, "en"),
        description_zh: localized(&raw.description, "zh"),
        raw_name_json: raw_json(&raw.name),
        raw_description_json: optional_raw_json(&raw.description),
    })
}

pub fn parse_group_line(line: &str) -> Result<CatalogGroup, SdeParseError> {
    let raw: RawGroupRow = parse_row("groups", line)?;
    Ok(CatalogGroup {
        group_id: raw.key,
        category_id: raw.category_id,
        published: raw.published,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        raw_name_json: raw_json(&raw.name),
    })
}

pub fn parse_category_line(line: &str) -> Result<CatalogCategory, SdeParseError> {
    let raw: RawCategoryRow = parse_row("categories", line)?;
    Ok(CatalogCategory {
        category_id: raw.key,
        published: raw.published,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        raw_name_json: raw_json(&raw.name),
    })
}

pub fn parse_market_group_line(line: &str) -> Result<CatalogMarketGroup, SdeParseError> {
    let raw: RawMarketGroupRow = parse_row("marketGroups", line)?;
    Ok(CatalogMarketGroup {
        market_group_id: raw.key,
        parent_group_id: raw.parent_group_id,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        description_en: localized(&raw.description, "en"),
        description_zh: localized(&raw.description, "zh"),
        raw_name_json: raw_json(&raw.name),
        raw_description_json: optional_raw_json(&raw.description),
    })
}

pub fn parse_sde_metadata_line(line: &str) -> Result<SdeMetadata, SdeParseError> {
    let raw: RawSdeMetadataRow = parse_row("_sde", line)?;
    Ok(SdeMetadata {
        build_number: raw.build_number,
        release_date: raw.release_date,
    })
}
```

- [ ] **Step 5: Implement archive reader**

Create `crates/sde/src/archive.rs`:

```rust
use crate::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};
use std::io::{BufRead, BufReader, Cursor, Read, Seek};
use thiserror::Error;
use zip::ZipArchive;

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogArchive {
    pub metadata: SdeMetadata,
    pub types: Vec<CatalogType>,
    pub groups: Vec<CatalogGroup>,
    pub categories: Vec<CatalogCategory>,
    pub market_groups: Vec<CatalogMarketGroup>,
}

#[derive(Debug, Error)]
pub enum SdeArchiveError {
    #[error("invalid SDE zip archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("failed to read SDE archive: {0}")]
    Io(#[from] std::io::Error),
    #[error("missing required SDE file {0}")]
    MissingRequiredFile(&'static str),
    #[error("failed to parse {file_name} line {line_number}: {source}")]
    ParseLine {
        file_name: &'static str,
        line_number: usize,
        #[source]
        source: crate::SdeParseError,
    },
}

pub fn read_catalog_archive_from_bytes(
    bytes: impl Into<Vec<u8>>,
) -> Result<CatalogArchive, SdeArchiveError> {
    read_catalog_archive_from_zip(ZipArchive::new(Cursor::new(bytes.into()))?)
}

fn required_lines<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &'static str,
) -> Result<Vec<String>, SdeArchiveError> {
    let file = zip
        .by_name(name)
        .map_err(|_| SdeArchiveError::MissingRequiredFile(name))?;
    BufReader::new(file)
        .lines()
        .collect::<Result<Vec<_>, _>>()
        .map_err(SdeArchiveError::from)
}

fn parse_lines<T>(
    name: &'static str,
    lines: Vec<String>,
    parse: fn(&str) -> Result<T, crate::SdeParseError>,
) -> Result<Vec<T>, SdeArchiveError> {
    lines
        .into_iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            parse(&line).map_err(|source| SdeArchiveError::ParseLine {
                file_name: name,
                line_number: index + 1,
                source,
            })
        })
        .collect()
}

fn read_catalog_archive_from_zip<R: Read + Seek>(
    mut zip: ZipArchive<R>,
) -> Result<CatalogArchive, SdeArchiveError> {
    let metadata_line = required_lines(&mut zip, "_sde.jsonl")?
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .ok_or(SdeArchiveError::MissingRequiredFile("_sde.jsonl"))?;
    let metadata =
        parse_sde_metadata_line(&metadata_line).map_err(|source| SdeArchiveError::ParseLine {
            file_name: "_sde.jsonl",
            line_number: 1,
            source,
        })?;

    Ok(CatalogArchive {
        metadata,
        types: parse_lines("types.jsonl", required_lines(&mut zip, "types.jsonl")?, parse_type_line)?,
        groups: parse_lines("groups.jsonl", required_lines(&mut zip, "groups.jsonl")?, parse_group_line)?,
        categories: parse_lines(
            "categories.jsonl",
            required_lines(&mut zip, "categories.jsonl")?,
            parse_category_line,
        )?,
        market_groups: parse_lines(
            "marketGroups.jsonl",
            required_lines(&mut zip, "marketGroups.jsonl")?,
            parse_market_group_line,
        )?,
    })
}
```

- [ ] **Step 6: Export SDE API**

Replace `crates/sde/src/lib.rs`:

```rust
pub mod archive;
pub mod models;
pub mod parser;

pub use archive::{read_catalog_archive_from_bytes, CatalogArchive, SdeArchiveError};
pub use models::{
    CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};
pub use parser::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, SdeParseError,
};
```

- [ ] **Step 7: Run tests**

Run:

```bash
cargo test -p evetools-sde --test parser
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/sde/src crates/sde/tests/parser.rs
git commit -m "feat: parse sde catalog archives"
```

## Task 3: SDE Download Client

**Files:**

- Create: `crates/sde/src/client.rs`
- Modify: `crates/sde/src/lib.rs`
- Create: `crates/sde/tests/client.rs`

- [ ] **Step 1: Write client tests**

Create `crates/sde/tests/client.rs`:

```rust
use evetools_sde::SdeClient;
use httpmock::prelude::*;

#[tokio::test]
async fn fetches_latest_metadata() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/static-data/tranquility/latest.jsonl");
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
    assert_eq!(bytes.as_ref(), b"zip-bytes");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p evetools-sde --test client
```

Expected: FAIL with unresolved `SdeClient`.

- [ ] **Step 3: Implement client**

Create `crates/sde/src/client.rs`:

```rust
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
```

- [ ] **Step 4: Export client API**

Modify `crates/sde/src/lib.rs`:

```rust
pub mod archive;
pub mod client;
pub mod models;
pub mod parser;

pub use archive::{read_catalog_archive_from_bytes, CatalogArchive, SdeArchiveError};
pub use client::{SdeClient, SdeClientError, SdeLatestMetadata};
pub use models::{
    CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};
pub use parser::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, SdeParseError,
};
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p evetools-sde
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/sde/src/client.rs crates/sde/src/lib.rs crates/sde/tests/client.rs Cargo.lock
git commit -m "feat: add sde download client"
```

## Task 4: Supabase/Postgres Schema And Repository

**Files:**

- Modify: `crates/db/Cargo.toml`
- Replace: `crates/db/src/lib.rs`
- Create: `crates/db/src/catalog.rs`
- Create: `crates/db/src/schema.rs`
- Create: `crates/db/tests/catalog_repository.rs`

- [ ] **Step 1: Add DB dependencies**

Modify `crates/db/Cargo.toml`:

```toml
[package]
name = "evetools-db"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
chrono.workspace = true
evetools-sde = { path = "../sde" }
serde.workspace = true
serde_json.workspace = true
sqlx.workspace = true
thiserror.workspace = true

[dev-dependencies]
tokio.workspace = true
```

- [ ] **Step 2: Write repository tests**

Create `crates/db/tests/catalog_repository.rs`:

```rust
use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogRepository, ImportCatalogInput,
};
use evetools_sde::{
    CatalogArchive, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};

fn database_url() -> Option<String> {
    std::env::var("EVETOOLS_TEST_DATABASE_URL").ok()
}

fn sample_archive() -> CatalogArchive {
    CatalogArchive {
        metadata: SdeMetadata {
            build_number: Some(3_351_823),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
        },
        types: vec![CatalogType {
            type_id: 34,
            group_id: 18,
            market_group_id: Some(1857),
            published: true,
            volume: Some(0.01),
            packaged_volume: Some(0.01),
            capacity: None,
            mass: Some(0.0),
            portion_size: Some(1),
            meta_level: None,
            name_en: Some("Tritanium".to_string()),
            name_zh: Some("三钛合金".to_string()),
            description_en: None,
            description_zh: None,
            raw_name_json: serde_json::json!({"en":"Tritanium","zh":"三钛合金"}),
            raw_description_json: None,
        }],
        groups: vec![CatalogGroup {
            group_id: 18,
            category_id: 4,
            published: true,
            name_en: Some("Mineral".to_string()),
            name_zh: Some("矿物".to_string()),
            raw_name_json: serde_json::json!({"en":"Mineral","zh":"矿物"}),
        }],
        categories: vec![CatalogCategory {
            category_id: 4,
            published: true,
            name_en: Some("Material".to_string()),
            name_zh: Some("材料".to_string()),
            raw_name_json: serde_json::json!({"en":"Material","zh":"材料"}),
        }],
        market_groups: vec![CatalogMarketGroup {
            market_group_id: 1857,
            parent_group_id: None,
            name_en: Some("Minerals".to_string()),
            name_zh: Some("矿物".to_string()),
            description_en: None,
            description_zh: None,
            raw_name_json: serde_json::json!({"en":"Minerals","zh":"矿物"}),
            raw_description_json: None,
        }],
    }
}

#[tokio::test]
async fn imports_and_searches_catalog_rows() {
    let Some(url) = database_url() else {
        eprintln!("skipping Postgres test: EVETOOLS_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = connect_pool(&url).await.unwrap();
    migrate_catalog_schema(&pool).await.unwrap();
    let repository = CatalogRepository::new(pool.clone());

    let status = repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "test://sample",
        })
        .await
        .unwrap();
    let zh = repository.get_inventory_type(34, "zh").await.unwrap().unwrap();
    let search = repository.search_inventory_types("三钛", "zh", 10).await.unwrap();

    assert_eq!(status.status, "success");
    assert_eq!(status.build_number, Some(3_351_823));
    assert_eq!(zh.display_name, "三钛合金");
    assert_eq!(search[0].type_id, 34);
}
```

- [ ] **Step 3: Run test and verify failure**

Run:

```bash
cargo test -p evetools-db imports_and_searches_catalog_rows
```

Expected: FAIL with unresolved DB symbols, or SKIP-style pass if `EVETOOLS_TEST_DATABASE_URL` is not set before code exists.

- [ ] **Step 4: Implement schema**

Create `crates/db/src/schema.rs`:

```rust
use sqlx::{PgPool, postgres::PgPoolOptions};

pub async fn connect_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

pub async fn migrate_catalog_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE SCHEMA IF NOT EXISTS evetools_catalog;

        CREATE TABLE IF NOT EXISTS evetools_catalog.sde_imports (
            import_id BIGSERIAL PRIMARY KEY,
            build_number INTEGER,
            release_date TEXT,
            source_url TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL,
            completed_at TIMESTAMPTZ,
            status TEXT NOT NULL,
            error_summary TEXT,
            type_count BIGINT NOT NULL DEFAULT 0,
            group_count BIGINT NOT NULL DEFAULT 0,
            category_count BIGINT NOT NULL DEFAULT 0,
            market_group_count BIGINT NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_types (
            type_id INTEGER PRIMARY KEY,
            group_id INTEGER NOT NULL,
            market_group_id INTEGER,
            published BOOLEAN NOT NULL,
            volume DOUBLE PRECISION,
            packaged_volume DOUBLE PRECISION,
            capacity DOUBLE PRECISION,
            mass DOUBLE PRECISION,
            portion_size INTEGER,
            meta_level INTEGER,
            name_en TEXT,
            name_zh TEXT,
            description_en TEXT,
            description_zh TEXT,
            raw_name_json JSONB NOT NULL,
            raw_description_json JSONB,
            updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
        );

        CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_groups (
            group_id INTEGER PRIMARY KEY,
            category_id INTEGER NOT NULL,
            published BOOLEAN NOT NULL,
            name_en TEXT,
            name_zh TEXT,
            raw_name_json JSONB NOT NULL,
            updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
        );

        CREATE TABLE IF NOT EXISTS evetools_catalog.inventory_categories (
            category_id INTEGER PRIMARY KEY,
            published BOOLEAN NOT NULL,
            name_en TEXT,
            name_zh TEXT,
            raw_name_json JSONB NOT NULL,
            updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
        );

        CREATE TABLE IF NOT EXISTS evetools_catalog.market_groups (
            market_group_id INTEGER PRIMARY KEY,
            parent_group_id INTEGER,
            name_en TEXT,
            name_zh TEXT,
            description_en TEXT,
            description_zh TEXT,
            raw_name_json JSONB NOT NULL,
            raw_description_json JSONB,
            updated_import_id BIGINT NOT NULL REFERENCES evetools_catalog.sde_imports(import_id)
        );

        CREATE INDEX IF NOT EXISTS idx_evetools_inventory_types_name_en
            ON evetools_catalog.inventory_types(name_en);
        CREATE INDEX IF NOT EXISTS idx_evetools_inventory_types_name_zh
            ON evetools_catalog.inventory_types(name_zh);
        CREATE INDEX IF NOT EXISTS idx_evetools_inventory_types_market_group
            ON evetools_catalog.inventory_types(market_group_id);
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 5: Implement repository**

Create `crates/db/src/catalog.rs`:

```rust
use chrono::Utc;
use evetools_sde::CatalogArchive;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogDbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogStatus {
    pub status: String,
    pub build_number: Option<i32>,
    pub release_date: Option<String>,
    pub source_url: Option<String>,
    pub completed_at: Option<String>,
    pub error_summary: Option<String>,
    pub type_count: i64,
    pub group_count: i64,
    pub category_count: i64,
    pub market_group_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryTypeView {
    pub type_id: i32,
    pub group_id: i32,
    pub category_id: Option<i32>,
    pub market_group_id: Option<i32>,
    pub display_name: String,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub group_name: Option<String>,
    pub category_name: Option<String>,
    pub market_group_name: Option<String>,
    pub published: bool,
    pub market_eligible: bool,
}

pub struct ImportCatalogInput<'a> {
    pub archive: &'a CatalogArchive,
    pub source_url: &'a str,
}

#[derive(Clone)]
pub struct CatalogRepository {
    pool: PgPool,
}

impl CatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn latest_status(&self) -> Result<CatalogStatus, CatalogDbError> {
        let row = sqlx::query_as::<_, (String, Option<i32>, Option<String>, Option<String>, Option<chrono::DateTime<Utc>>, Option<String>, i64, i64, i64, i64)>(
            "SELECT status, build_number, release_date, source_url, completed_at, error_summary,
                    type_count, group_count, category_count, market_group_count
             FROM evetools_catalog.sde_imports
             ORDER BY import_id DESC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| CatalogStatus {
            status: row.0,
            build_number: row.1,
            release_date: row.2,
            source_url: row.3,
            completed_at: row.4.map(|value| value.to_rfc3339()),
            error_summary: row.5,
            type_count: row.6,
            group_count: row.7,
            category_count: row.8,
            market_group_count: row.9,
        }).unwrap_or(CatalogStatus {
            status: "not-imported".to_string(),
            build_number: None,
            release_date: None,
            source_url: None,
            completed_at: None,
            error_summary: None,
            type_count: 0,
            group_count: 0,
            category_count: 0,
            market_group_count: 0,
        }))
    }

    pub async fn import_archive(
        &self,
        input: ImportCatalogInput<'_>,
    ) -> Result<CatalogStatus, CatalogDbError> {
        let mut tx = self.pool.begin().await?;
        let import_id: i64 = sqlx::query_scalar(
            "INSERT INTO evetools_catalog.sde_imports
                (build_number, release_date, source_url, started_at, status)
             VALUES ($1, $2, $3, NOW(), 'running')
             RETURNING import_id",
        )
        .bind(input.archive.metadata.build_number)
        .bind(input.archive.metadata.release_date.as_deref())
        .bind(input.source_url)
        .fetch_one(&mut *tx)
        .await?;

        insert_categories(&mut tx, import_id, input.archive).await?;
        insert_groups(&mut tx, import_id, input.archive).await?;
        insert_market_groups(&mut tx, import_id, input.archive).await?;
        insert_types(&mut tx, import_id, input.archive).await?;

        sqlx::query(
            "UPDATE evetools_catalog.sde_imports
             SET completed_at = NOW(), status = 'success',
                 type_count = $1, group_count = $2,
                 category_count = $3, market_group_count = $4
             WHERE import_id = $5",
        )
        .bind(input.archive.types.len() as i64)
        .bind(input.archive.groups.len() as i64)
        .bind(input.archive.categories.len() as i64)
        .bind(input.archive.market_groups.len() as i64)
        .bind(import_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.latest_status().await
    }

    pub async fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, CatalogDbError> {
        let row = sqlx::query_as::<_, InventoryTypeRow>(TYPE_SELECT_SQL)
            .bind(type_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.into_view(language)))
    }

    pub async fn search_inventory_types(
        &self,
        query: &str,
        language: &str,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, CatalogDbError> {
        let pattern = format!("%{}%", query.trim());
        let rows = sqlx::query_as::<_, InventoryTypeRow>(
            "SELECT t.type_id, t.group_id, g.category_id, t.market_group_id,
                    t.name_en, t.name_zh, g.name_en AS group_name_en, g.name_zh AS group_name_zh,
                    c.name_en AS category_name_en, c.name_zh AS category_name_zh,
                    mg.name_en AS market_group_name_en, mg.name_zh AS market_group_name_zh,
                    t.published,
                    (t.published AND t.market_group_id IS NOT NULL AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)) AS market_eligible
             FROM evetools_catalog.inventory_types t
             LEFT JOIN evetools_catalog.inventory_groups g ON g.group_id = t.group_id
             LEFT JOIN evetools_catalog.inventory_categories c ON c.category_id = g.category_id
             LEFT JOIN evetools_catalog.market_groups mg ON mg.market_group_id = t.market_group_id
             WHERE t.name_en ILIKE $1 OR t.name_zh ILIKE $1
             ORDER BY t.name_en NULLS LAST
             LIMIT $2",
        )
        .bind(pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|row| row.into_view(language)).collect())
    }
}

const TYPE_SELECT_SQL: &str = "SELECT t.type_id, t.group_id, g.category_id, t.market_group_id,
        t.name_en, t.name_zh, g.name_en AS group_name_en, g.name_zh AS group_name_zh,
        c.name_en AS category_name_en, c.name_zh AS category_name_zh,
        mg.name_en AS market_group_name_en, mg.name_zh AS market_group_name_zh,
        t.published,
        (t.published AND t.market_group_id IS NOT NULL AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)) AS market_eligible
    FROM evetools_catalog.inventory_types t
    LEFT JOIN evetools_catalog.inventory_groups g ON g.group_id = t.group_id
    LEFT JOIN evetools_catalog.inventory_categories c ON c.category_id = g.category_id
    LEFT JOIN evetools_catalog.market_groups mg ON mg.market_group_id = t.market_group_id
    WHERE t.type_id = $1";

#[derive(sqlx::FromRow)]
struct InventoryTypeRow {
    type_id: i32,
    group_id: i32,
    category_id: Option<i32>,
    market_group_id: Option<i32>,
    name_en: Option<String>,
    name_zh: Option<String>,
    group_name_en: Option<String>,
    group_name_zh: Option<String>,
    category_name_en: Option<String>,
    category_name_zh: Option<String>,
    market_group_name_en: Option<String>,
    market_group_name_zh: Option<String>,
    published: bool,
    market_eligible: bool,
}

impl InventoryTypeRow {
    fn into_view(self, language: &str) -> InventoryTypeView {
        let prefer_zh = language.starts_with("zh");
        let display_name = choose_name(prefer_zh, self.name_zh.as_ref(), self.name_en.as_ref())
            .unwrap_or_else(|| format!("Type {}", self.type_id));
        InventoryTypeView {
            type_id: self.type_id,
            group_id: self.group_id,
            category_id: self.category_id,
            market_group_id: self.market_group_id,
            display_name,
            name_en: self.name_en,
            name_zh: self.name_zh,
            group_name: choose_name(prefer_zh, self.group_name_zh.as_ref(), self.group_name_en.as_ref()),
            category_name: choose_name(prefer_zh, self.category_name_zh.as_ref(), self.category_name_en.as_ref()),
            market_group_name: choose_name(prefer_zh, self.market_group_name_zh.as_ref(), self.market_group_name_en.as_ref()),
            published: self.published,
            market_eligible: self.market_eligible,
        }
    }
}

fn choose_name(prefer_zh: bool, zh: Option<&String>, en: Option<&String>) -> Option<String> {
    if prefer_zh {
        zh.or(en).cloned()
    } else {
        en.or(zh).cloned()
    }
}

async fn insert_categories(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.categories {
        sqlx::query(
            "INSERT INTO evetools_catalog.inventory_categories
                (category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (category_id) DO UPDATE SET
                published = EXCLUDED.published,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.category_id)
        .bind(row.published)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
```

Add the remaining repository insert helpers to `crates/db/src/catalog.rs`:

```rust
async fn insert_groups(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.groups {
        sqlx::query(
            "INSERT INTO evetools_catalog.inventory_groups
                (group_id, category_id, published, name_en, name_zh, raw_name_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (group_id) DO UPDATE SET
                category_id = EXCLUDED.category_id,
                published = EXCLUDED.published,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.group_id)
        .bind(row.category_id)
        .bind(row.published)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_market_groups(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.market_groups {
        sqlx::query(
            "INSERT INTO evetools_catalog.market_groups
                (market_group_id, parent_group_id, name_en, name_zh, description_en, description_zh,
                 raw_name_json, raw_description_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (market_group_id) DO UPDATE SET
                parent_group_id = EXCLUDED.parent_group_id,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                description_en = EXCLUDED.description_en,
                description_zh = EXCLUDED.description_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                raw_description_json = EXCLUDED.raw_description_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.market_group_id)
        .bind(row.parent_group_id)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(row.description_en.as_deref())
        .bind(row.description_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(row.raw_description_json.as_ref())
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_types(
    tx: &mut Transaction<'_, Postgres>,
    import_id: i64,
    archive: &CatalogArchive,
) -> Result<(), sqlx::Error> {
    for row in &archive.types {
        sqlx::query(
            "INSERT INTO evetools_catalog.inventory_types
                (type_id, group_id, market_group_id, published, volume, packaged_volume, capacity,
                 mass, portion_size, meta_level, name_en, name_zh, description_en, description_zh,
                 raw_name_json, raw_description_json, updated_import_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
             ON CONFLICT (type_id) DO UPDATE SET
                group_id = EXCLUDED.group_id,
                market_group_id = EXCLUDED.market_group_id,
                published = EXCLUDED.published,
                volume = EXCLUDED.volume,
                packaged_volume = EXCLUDED.packaged_volume,
                capacity = EXCLUDED.capacity,
                mass = EXCLUDED.mass,
                portion_size = EXCLUDED.portion_size,
                meta_level = EXCLUDED.meta_level,
                name_en = EXCLUDED.name_en,
                name_zh = EXCLUDED.name_zh,
                description_en = EXCLUDED.description_en,
                description_zh = EXCLUDED.description_zh,
                raw_name_json = EXCLUDED.raw_name_json,
                raw_description_json = EXCLUDED.raw_description_json,
                updated_import_id = EXCLUDED.updated_import_id",
        )
        .bind(row.type_id)
        .bind(row.group_id)
        .bind(row.market_group_id)
        .bind(row.published)
        .bind(row.volume)
        .bind(row.packaged_volume)
        .bind(row.capacity)
        .bind(row.mass)
        .bind(row.portion_size)
        .bind(row.meta_level)
        .bind(row.name_en.as_deref())
        .bind(row.name_zh.as_deref())
        .bind(row.description_en.as_deref())
        .bind(row.description_zh.as_deref())
        .bind(&row.raw_name_json)
        .bind(row.raw_description_json.as_ref())
        .bind(import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
```

- [ ] **Step 6: Export DB API**

Replace `crates/db/src/lib.rs`:

```rust
pub mod catalog;
pub mod schema;

pub use catalog::{
    CatalogDbError, CatalogRepository, CatalogStatus, ImportCatalogInput, InventoryTypeView,
};
pub use schema::{connect_pool, migrate_catalog_schema};

pub fn storage_mode() -> &'static str {
    "supabase-postgres-catalog"
}
```

- [ ] **Step 7: Run DB tests**

Run:

```bash
cargo test -p evetools-db
```

Expected: PASS. If `EVETOOLS_TEST_DATABASE_URL` is unset, integration test logs a skip message and returns success.

- [ ] **Step 8: Commit**

```bash
git add crates/db/Cargo.toml crates/db/src crates/db/tests/catalog_repository.rs Cargo.lock
git commit -m "feat: store sde catalog in postgres"
```

## Task 5: Catalog Application Service

**Files:**

- Replace: `crates/catalog/src/lib.rs`
- Create: `crates/catalog/tests/service.rs`

- [ ] **Step 1: Write service config test**

Create `crates/catalog/tests/service.rs`:

```rust
use evetools_catalog::{CatalogConfig, CatalogServiceError};

#[test]
fn config_requires_database_url() {
    let error = CatalogConfig::from_database_url("").unwrap_err();

    assert!(matches!(error, CatalogServiceError::MissingDatabaseUrl));
}
```

- [ ] **Step 2: Run test and verify failure**

Run:

```bash
cargo test -p evetools-catalog config_requires_database_url
```

Expected: FAIL with unresolved `CatalogConfig`.

- [ ] **Step 3: Implement service**

Replace `crates/catalog/src/lib.rs`:

```rust
use evetools_db::{
    connect_pool, migrate_catalog_schema, CatalogRepository, CatalogStatus, ImportCatalogInput,
    InventoryTypeView,
};
use evetools_sde::{read_catalog_archive_from_bytes, SdeClient};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct CatalogConfig {
    pub database_url: String,
}

#[derive(Debug, Error)]
pub enum CatalogServiceError {
    #[error("EVETOOLS_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("database error: {0}")]
    Database(#[from] evetools_db::CatalogDbError),
    #[error("sql migration or connection error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("SDE download error: {0}")]
    SdeClient(#[from] evetools_sde::SdeClientError),
    #[error("SDE archive error: {0}")]
    SdeArchive(#[from] evetools_sde::SdeArchiveError),
}

impl CatalogConfig {
    pub fn from_database_url(value: impl Into<String>) -> Result<Self, CatalogServiceError> {
        let database_url = value.into();
        if database_url.trim().is_empty() {
            return Err(CatalogServiceError::MissingDatabaseUrl);
        }
        Ok(Self { database_url })
    }

    pub fn from_env() -> Result<Self, CatalogServiceError> {
        Self::from_database_url(std::env::var("EVETOOLS_DATABASE_URL").unwrap_or_default())
    }
}

pub struct CatalogService {
    repository: CatalogRepository,
}

impl CatalogService {
    pub async fn connect(config: CatalogConfig) -> Result<Self, CatalogServiceError> {
        let pool = connect_pool(&config.database_url).await?;
        migrate_catalog_schema(&pool).await?;
        Ok(Self {
            repository: CatalogRepository::new(pool),
        })
    }

    pub async fn status(&self) -> Result<CatalogStatus, CatalogServiceError> {
        Ok(self.repository.latest_status().await?)
    }

    pub async fn import_latest(&self) -> Result<CatalogStatus, CatalogServiceError> {
        let client = SdeClient::official()?;
        let bytes = client.download_latest_archive().await?;
        let archive = read_catalog_archive_from_bytes(bytes)?;
        Ok(self
            .repository
            .import_archive(ImportCatalogInput {
                archive: &archive,
                source_url: "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip",
            })
            .await?)
    }

    pub async fn search_inventory_types(
        &self,
        query: &str,
        language: &str,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, CatalogServiceError> {
        Ok(self
            .repository
            .search_inventory_types(query, language, limit)
            .await?)
    }

    pub async fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, CatalogServiceError> {
        Ok(self.repository.get_inventory_type(type_id, language).await?)
    }
}
```

- [ ] **Step 4: Add missing dependency**

Modify `crates/catalog/Cargo.toml` dependencies:

```toml
[dependencies]
evetools-db = { path = "../db" }
evetools-sde = { path = "../sde" }
serde.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tokio.workspace = true
```

- [ ] **Step 5: Run catalog tests**

Run:

```bash
cargo test -p evetools-catalog
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/catalog/Cargo.toml crates/catalog/src/lib.rs crates/catalog/tests/service.rs Cargo.lock
git commit -m "feat: add catalog service boundary"
```

## Task 6: Tauri Commands And TypeScript Wrappers

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/commands.ts`

- [ ] **Step 1: Add desktop dependency**

Modify `apps/desktop/src-tauri/Cargo.toml` dependencies:

```toml
evetools-catalog = { path = "../../../crates/catalog" }
```

Keep existing desktop dependencies.

- [ ] **Step 2: Add Tauri command helpers**

Add imports in `apps/desktop/src-tauri/src/lib.rs`:

```rust
use evetools_catalog::{CatalogConfig, CatalogService};
use evetools_db::{CatalogStatus, InventoryTypeView};
```

Add command helpers before `pub fn run()`:

```rust
async fn catalog_service_from_env() -> Result<CatalogService, String> {
    let config = CatalogConfig::from_env().map_err(|error| error.to_string())?;
    CatalogService::connect(config)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_sde_catalog_status() -> Result<CatalogStatus, String> {
    catalog_service_from_env().await?.status().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn import_sde_catalog_latest() -> Result<CatalogStatus, String> {
    catalog_service_from_env()
        .await?
        .import_latest()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn search_inventory_types(
    query: String,
    language: String,
    limit: i64,
) -> Result<Vec<InventoryTypeView>, String> {
    catalog_service_from_env()
        .await?
        .search_inventory_types(&query, &language, limit)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_inventory_type(
    type_id: i32,
    language: String,
) -> Result<Option<InventoryTypeView>, String> {
    catalog_service_from_env()
        .await?
        .get_inventory_type(type_id, &language)
        .await
        .map_err(|error| error.to_string())
}
```

Register commands in `tauri::generate_handler!`:

```rust
            get_sde_catalog_status,
            import_sde_catalog_latest,
            search_inventory_types,
            get_inventory_type
```

- [ ] **Step 3: Add TypeScript wrappers**

Append to `apps/desktop/src/commands.ts`:

```ts
export type CatalogStatus = {
  status: string;
  build_number: number | null;
  release_date: string | null;
  source_url: string | null;
  completed_at: string | null;
  error_summary: string | null;
  type_count: number;
  group_count: number;
  category_count: number;
  market_group_count: number;
};

export type InventoryTypeView = {
  type_id: number;
  group_id: number;
  category_id: number | null;
  market_group_id: number | null;
  display_name: string;
  name_en: string | null;
  name_zh: string | null;
  group_name: string | null;
  category_name: string | null;
  market_group_name: string | null;
  published: boolean;
  market_eligible: boolean;
};

export function getSdeCatalogStatus(): Promise<CatalogStatus> {
  return invoke<CatalogStatus>("get_sde_catalog_status");
}

export function importSdeCatalogLatest(): Promise<CatalogStatus> {
  return invoke<CatalogStatus>("import_sde_catalog_latest");
}

export function searchInventoryTypes(
  query: string,
  language: string,
  limit = 20
): Promise<InventoryTypeView[]> {
  return invoke<InventoryTypeView[]>("search_inventory_types", { query, language, limit });
}

export function getInventoryType(
  typeId: number,
  language: string
): Promise<InventoryTypeView | null> {
  return invoke<InventoryTypeView | null>("get_inventory_type", { typeId, language });
}
```

- [ ] **Step 4: Verify desktop compilation**

Run:

```bash
cargo test -p evetools-desktop
pnpm --filter @evetools/desktop typecheck
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/lib.rs apps/desktop/src/commands.ts Cargo.lock
git commit -m "feat: expose catalog service commands"
```

## Task 7: README And Secret Handling Documentation

**Files:**

- Modify: `README.md`

- [ ] **Step 1: Add Supabase catalog docs**

Add:

````markdown
## Static SDE Catalog

EveTools imports CCP's official SDE JSON Lines archive into Supabase Postgres through the Rust catalog service.

Required environment variable:

```bash
export EVETOOLS_DATABASE_URL="<supabase-postgres-url-with-sslmode-require>"
```

Do not commit real database URLs or passwords. If a credential is pasted into chat, logs, or source control, rotate it in Supabase before use.

The first catalog slice imports:

- `_sde.jsonl`
- `types.jsonl`
- `groups.jsonl`
- `categories.jsonl`
- `marketGroups.jsonl`

React does not connect to Supabase directly. It calls Tauri commands, and Tauri calls the Rust catalog service.
````

- [ ] **Step 2: Verify docs**

Run:

```bash
rg -n "Static SDE Catalog|EVETOOLS_DATABASE_URL|Supabase|React does not connect" README.md
```

Expected: all terms found.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document supabase sde catalog"
```

## Task 8: Final Verification

**Files:**

- No edits expected.

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 2: Rust tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Workspace check**

Run:

```bash
pnpm check
```

Expected: PASS.

- [ ] **Step 4: Secret scan**

Run:

```bash
rg -n 'postgres(ql)?://[^[:space:]]+:[^[:space:]]+@|db\.[a-z0-9-]+\.supabase\.co|pooler\.supabase\.com' . \
  -g '!docs/superpowers/plans/2026-05-26-sde-static-catalog-import.md'
```

Expected: no matches. Real credentials must not appear.

## Self-Review Checklist

- Spec coverage: Supabase-first storage, Rust service boundary, parser, importer, repository, commands, and docs are all mapped to tasks.
- Secret handling: plan uses only `EVETOOLS_DATABASE_URL` and placeholder URLs.
- UI boundary: React only receives typed Tauri command results.
- Known deferral: distributed app credential hardening requires a hosted API or strict RLS-backed design before public release.
