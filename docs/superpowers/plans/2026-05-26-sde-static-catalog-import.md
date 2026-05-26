# SDE Static Catalog Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a local static EVE catalog importer that reads official SDE JSONL zip archives into SQLite and exposes localized item lookup/search for later market discovery.

**Architecture:** Add a focused `crates/sde` crate for SDE zip reading, JSONL row parsing, latest metadata lookup, and archive download. Expand `crates/db` into a synchronous `rusqlite` catalog repository that owns schema, transactional imports, and search. Keep Tauri command handlers thin wrappers over these services; no SDE parsing or SQL construction should live in the desktop command file.

**Tech Stack:** Rust 1.82 workspace, `zip` for SDE archives, `serde`/`serde_json` for JSONL parsing, `reqwest` for official latest metadata/download, `rusqlite` with bundled SQLite for local storage, `tempfile` for tests, Tauri 2 commands, React/Vite/i18next type wrappers.

---

## Scope

This plan implements the catalog MVP from [2026-05-26-sde-static-catalog-import-design.md](../specs/2026-05-26-sde-static-catalog-import-design.md).

In scope:

- Parse `_sde.jsonl`, `types.jsonl`, `groups.jsonl`, `categories.jsonl`, and `marketGroups.jsonl`.
- Normalize `_key` into internal IDs.
- Preserve English and Chinese names/descriptions with raw localized JSON.
- Import a local SDE JSONL zip into SQLite transactionally.
- Query catalog status, type lookup, type search, and market-eligible types.
- Download official latest SDE metadata and archive through reusable service functions.
- Add Tauri commands and TypeScript wrappers for status, local import, latest import, search, and lookup.
- Document development usage.

Out of scope:

- Dogma, blueprints, icons, full universe data, and delta SDE change files.
- A polished settings/update UI.
- NPC Hub Selection Discovery consumption of the catalog. That comes after this foundation.

## Technology Choices

- Use `rusqlite` instead of `sqlx` for this phase. The import path is synchronous, transaction-heavy, local-only, and does not need async connection pooling.
- Use `zip = "5"` with `default-features = false` and `features = ["deflate"]`. The official JSONL zip uses ordinary deflate compression, and this avoids pulling unnecessary archive codecs.
- Use `rusqlite = { version = "0.40", features = ["bundled"] }` so desktop behavior does not depend on the host SQLite version.
- Start search with indexed `LIKE` over `name_en` and `name_zh`. Do not add FTS5 in this phase.

## File Structure

- Modify `Cargo.toml`: add workspace dependencies and workspace member `crates/sde`.
- Create `crates/sde/Cargo.toml`: SDE crate dependencies.
- Create `crates/sde/src/lib.rs`: public exports.
- Create `crates/sde/src/models.rs`: parsed SDE row structs and normalized catalog records.
- Create `crates/sde/src/parser.rs`: JSONL parsing, language normalization, and zip file reading.
- Create `crates/sde/src/client.rs`: official latest metadata and archive download.
- Create `crates/sde/tests/fixtures.rs`: helper for in-memory test archives.
- Create `crates/sde/tests/parser.rs`: parser and archive reader tests.
- Modify `crates/db/Cargo.toml`: add `rusqlite`, `serde`, `serde_json`, `chrono`, and dev `tempfile`.
- Replace `crates/db/src/lib.rs`: export database modules while keeping existing `storage_mode`.
- Create `crates/db/src/catalog.rs`: catalog models, status/search view structs, and repository methods.
- Create `crates/db/src/schema.rs`: SQLite schema migration.
- Create `crates/db/tests/catalog_repository.rs`: transactional import and repository tests.
- Modify `apps/desktop/src-tauri/Cargo.toml`: depend on `evetools-db` and `evetools-sde`.
- Modify `apps/desktop/src-tauri/src/lib.rs`: add thin catalog commands.
- Modify `apps/desktop/src/commands.ts`: add TypeScript catalog command wrappers.
- Modify `README.md`: document SDE catalog import.

## Task 1: Add Workspace Dependencies And SDE Crate Skeleton

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/sde/Cargo.toml`
- Create: `crates/sde/src/lib.rs`

- [ ] **Step 1: Add workspace dependencies**

Modify root `Cargo.toml` so the workspace members and dependencies include:

```toml
[workspace]
resolver = "2"
members = [
  "crates/domain",
  "crates/esi",
  "crates/db",
  "crates/sde",
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
rusqlite = { version = "0.40", features = ["bundled"] }
rust_decimal = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2" }
tauri-build = { version = "2" }
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
zip = { version = "5", default-features = false, features = ["deflate"] }
```

- [ ] **Step 2: Create SDE crate manifest**

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
zip.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 3: Create initial SDE library**

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

- [ ] **Step 4: Run SDE crate test**

Run:

```bash
cargo test -p evetools-sde
```

Expected: PASS with `sde_crate_reports_ready`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/sde/Cargo.toml crates/sde/src/lib.rs Cargo.lock
git commit -m "feat: add sde crate skeleton"
```

## Task 2: Parse SDE JSONL Rows

**Files:**

- Modify: `crates/sde/src/lib.rs`
- Create: `crates/sde/src/models.rs`
- Create: `crates/sde/src/parser.rs`
- Create: `crates/sde/tests/parser.rs`

- [ ] **Step 1: Write parser tests**

Create `crates/sde/tests/parser.rs`:

```rust
use evetools_sde::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};

#[test]
fn parses_type_line_and_normalizes_localized_names() {
    let line = r#"{"_key":34,"description":{"en":"Primary building block","zh":"主要建造材料"},"groupID":18,"marketGroupID":1857,"mass":0.0,"name":{"en":"Tritanium","zh":"三钛合金"},"packagedVolume":0.01,"portionSize":1,"published":true,"volume":0.01}"#;

    let parsed: CatalogType = parse_type_line(line).unwrap();

    assert_eq!(parsed.type_id, 34);
    assert_eq!(parsed.group_id, 18);
    assert_eq!(parsed.market_group_id, Some(1857));
    assert!(parsed.published);
    assert_eq!(parsed.name_en.as_deref(), Some("Tritanium"));
    assert_eq!(parsed.name_zh.as_deref(), Some("三钛合金"));
    assert_eq!(parsed.description_en.as_deref(), Some("Primary building block"));
    assert_eq!(parsed.description_zh.as_deref(), Some("主要建造材料"));
    assert_eq!(parsed.volume, Some(0.01));
    assert_eq!(parsed.packaged_volume, Some(0.01));
    assert!(parsed.raw_name_json.contains("Tritanium"));
}

#[test]
fn parses_type_line_with_missing_optional_fields() {
    let line = r#"{"_key":35,"groupID":18,"name":{"en":"Pyerite"},"portionSize":1,"published":true}"#;

    let parsed = parse_type_line(line).unwrap();

    assert_eq!(parsed.type_id, 35);
    assert_eq!(parsed.market_group_id, None);
    assert_eq!(parsed.name_en.as_deref(), Some("Pyerite"));
    assert_eq!(parsed.name_zh, None);
    assert_eq!(parsed.volume, None);
}

#[test]
fn parses_group_category_and_market_group_lines() {
    let group: CatalogGroup =
        parse_group_line(r#"{"_key":18,"categoryID":4,"name":{"en":"Mineral","zh":"矿物"},"published":true}"#)
            .unwrap();
    let category: CatalogCategory =
        parse_category_line(r#"{"_key":4,"name":{"en":"Material","zh":"材料"},"published":true}"#)
            .unwrap();
    let market_group: CatalogMarketGroup =
        parse_market_group_line(r#"{"_key":1857,"description":{"en":"Raw materials"},"name":{"en":"Minerals","zh":"矿物"},"parentGroupID":1031}"#)
            .unwrap();

    assert_eq!(group.group_id, 18);
    assert_eq!(group.category_id, 4);
    assert_eq!(group.name_zh.as_deref(), Some("矿物"));
    assert_eq!(category.category_id, 4);
    assert_eq!(category.name_en.as_deref(), Some("Material"));
    assert_eq!(market_group.market_group_id, 1857);
    assert_eq!(market_group.parent_group_id, Some(1031));
}

#[test]
fn parses_sde_metadata_line() {
    let parsed: SdeMetadata =
        parse_sde_metadata_line(r#"{"_key":"sde","buildNumber":3351823,"releaseDate":"2026-05-19T12:12:31Z"}"#)
            .unwrap();

    assert_eq!(parsed.build_number, Some(3_351_823));
    assert_eq!(parsed.release_date.as_deref(), Some("2026-05-19T12:12:31Z"));
}

#[test]
fn rejects_rows_missing_required_ids() {
    let error = parse_type_line(r#"{"groupID":18,"name":{"en":"Broken"},"published":true}"#)
        .unwrap_err();

    assert!(error.to_string().contains("missing field `_key`"));
}
```

- [ ] **Step 2: Run parser tests and verify they fail**

Run:

```bash
cargo test -p evetools-sde --test parser
```

Expected: FAIL with unresolved imports for parser functions and catalog structs.

- [ ] **Step 3: Implement models**

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
    pub raw_name_json: String,
    pub raw_description_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogGroup {
    pub group_id: i32,
    pub category_id: i32,
    pub published: bool,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub raw_name_json: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogCategory {
    pub category_id: i32,
    pub published: bool,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub raw_name_json: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogMarketGroup {
    pub market_group_id: i32,
    pub parent_group_id: Option<i32>,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub description_en: Option<String>,
    pub description_zh: Option<String>,
    pub raw_name_json: String,
    pub raw_description_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdeMetadata {
    pub build_number: Option<i32>,
    pub release_date: Option<String>,
}

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
    pub name: BTreeMap<String, String>,
    #[serde(default)]
    pub description: BTreeMap<String, String>,
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
    pub name: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawCategoryRow {
    #[serde(rename = "_key")]
    pub key: i32,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub name: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawMarketGroupRow {
    #[serde(rename = "_key")]
    pub key: i32,
    pub parent_group_id: Option<i32>,
    #[serde(default)]
    pub name: BTreeMap<String, String>,
    #[serde(default)]
    pub description: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSdeMetadataRow {
    #[serde(rename = "_key")]
    #[allow(dead_code)]
    pub key: String,
    pub build_number: Option<i32>,
    pub release_date: Option<String>,
}
```

- [ ] **Step 4: Implement parser functions**

Create `crates/sde/src/parser.rs`:

```rust
use crate::models::{
    CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, RawCategoryRow, RawGroupRow,
    RawMarketGroupRow, RawSdeMetadataRow, RawTypeRow, SdeMetadata,
};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
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

fn localized_value(values: &BTreeMap<String, String>, language: &str) -> Option<String> {
    values.get(language).filter(|value| !value.is_empty()).cloned()
}

fn raw_json(values: &BTreeMap<String, String>) -> String {
    serde_json::to_string(values).expect("localized string map should serialize")
}

fn optional_raw_json(values: &BTreeMap<String, String>) -> Option<String> {
    if values.is_empty() {
        None
    } else {
        Some(raw_json(values))
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
        name_en: localized_value(&raw.name, "en"),
        name_zh: localized_value(&raw.name, "zh"),
        description_en: localized_value(&raw.description, "en"),
        description_zh: localized_value(&raw.description, "zh"),
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
        name_en: localized_value(&raw.name, "en"),
        name_zh: localized_value(&raw.name, "zh"),
        raw_name_json: raw_json(&raw.name),
    })
}

pub fn parse_category_line(line: &str) -> Result<CatalogCategory, SdeParseError> {
    let raw: RawCategoryRow = parse_row("categories", line)?;
    Ok(CatalogCategory {
        category_id: raw.key,
        published: raw.published,
        name_en: localized_value(&raw.name, "en"),
        name_zh: localized_value(&raw.name, "zh"),
        raw_name_json: raw_json(&raw.name),
    })
}

pub fn parse_market_group_line(line: &str) -> Result<CatalogMarketGroup, SdeParseError> {
    let raw: RawMarketGroupRow = parse_row("marketGroups", line)?;
    Ok(CatalogMarketGroup {
        market_group_id: raw.key,
        parent_group_id: raw.parent_group_id,
        name_en: localized_value(&raw.name, "en"),
        name_zh: localized_value(&raw.name, "zh"),
        description_en: localized_value(&raw.description, "en"),
        description_zh: localized_value(&raw.description, "zh"),
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

- [ ] **Step 5: Export parser API**

Replace `crates/sde/src/lib.rs` with:

```rust
pub mod models;
pub mod parser;

pub use models::{CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata};
pub use parser::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, SdeParseError,
};
```

- [ ] **Step 6: Run parser tests**

Run:

```bash
cargo test -p evetools-sde --test parser
```

Expected: PASS with five parser tests.

- [ ] **Step 7: Commit**

```bash
git add crates/sde/src/lib.rs crates/sde/src/models.rs crates/sde/src/parser.rs crates/sde/tests/parser.rs
git commit -m "feat: parse sde catalog rows"
```

## Task 3: Read Required Files From SDE Zip Archives

**Files:**

- Modify: `crates/sde/src/lib.rs`
- Modify: `crates/sde/src/parser.rs`
- Create: `crates/sde/src/archive.rs`
- Create: `crates/sde/tests/fixtures.rs`
- Modify: `crates/sde/tests/parser.rs`

- [ ] **Step 1: Write archive helper for tests**

Create `crates/sde/tests/fixtures.rs`:

```rust
use std::fs::File;
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn write_test_sde_zip(path: &Path) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("_sde.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":"sde","buildNumber":3351823,"releaseDate":"2026-05-19T12:12:31Z"}}"#
    )
    .unwrap();

    zip.start_file("types.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":34,"description":{{"en":"Primary building block","zh":"主要建造材料"}},"groupID":18,"marketGroupID":1857,"mass":0.0,"name":{{"en":"Tritanium","zh":"三钛合金"}},"packagedVolume":0.01,"portionSize":1,"published":true,"volume":0.01}}"#
    )
    .unwrap();
    writeln!(
        zip,
        r#"{{"_key":35,"groupID":18,"name":{{"en":"Pyerite","zh":"类晶体胶矿"}},"portionSize":1,"published":true}}"#
    )
    .unwrap();

    zip.start_file("groups.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":18,"categoryID":4,"name":{{"en":"Mineral","zh":"矿物"}},"published":true}}"#
    )
    .unwrap();

    zip.start_file("categories.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":4,"name":{{"en":"Material","zh":"材料"}},"published":true}}"#
    )
    .unwrap();

    zip.start_file("marketGroups.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":1857,"description":{{"en":"Raw materials"}},"name":{{"en":"Minerals","zh":"矿物"}},"parentGroupID":1031}}"#
    )
    .unwrap();

    zip.finish().unwrap();
}

pub fn write_zip_missing_types(path: &Path) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("_sde.jsonl", options).unwrap();
    writeln!(
        zip,
        r#"{{"_key":"sde","buildNumber":3351823,"releaseDate":"2026-05-19T12:12:31Z"}}"#
    )
    .unwrap();

    zip.finish().unwrap();
}
```

- [ ] **Step 2: Add archive tests**

Append to `crates/sde/tests/parser.rs`:

```rust
mod fixtures;

use evetools_sde::{read_catalog_archive, CatalogArchive};
use tempfile::NamedTempFile;

#[test]
fn reads_catalog_archive_from_zip() {
    let archive_file = NamedTempFile::new().unwrap();
    fixtures::write_test_sde_zip(archive_file.path());

    let archive: CatalogArchive = read_catalog_archive(archive_file.path()).unwrap();

    assert_eq!(archive.metadata.build_number, Some(3_351_823));
    assert_eq!(archive.types.len(), 2);
    assert_eq!(archive.groups.len(), 1);
    assert_eq!(archive.categories.len(), 1);
    assert_eq!(archive.market_groups.len(), 1);
    assert_eq!(archive.types[0].name_en.as_deref(), Some("Tritanium"));
}

#[test]
fn missing_required_archive_file_is_error() {
    let archive_file = NamedTempFile::new().unwrap();
    fixtures::write_zip_missing_types(archive_file.path());

    let error = read_catalog_archive(archive_file.path()).unwrap_err();

    assert!(error.to_string().contains("missing required SDE file types.jsonl"));
}
```

- [ ] **Step 3: Run archive tests and verify they fail**

Run:

```bash
cargo test -p evetools-sde --test parser archive
```

Expected: FAIL with unresolved imports for `read_catalog_archive` and `CatalogArchive`.

- [ ] **Step 4: Implement archive reader**

Create `crates/sde/src/archive.rs`:

```rust
use crate::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use thiserror::Error;
use zip::result::ZipError;
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
    #[error("failed to open SDE archive: {0}")]
    Open(#[from] std::io::Error),
    #[error("invalid SDE zip archive: {0}")]
    Zip(#[from] ZipError),
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

fn read_required_lines<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    file_name: &'static str,
) -> Result<Vec<String>, SdeArchiveError> {
    let file = zip
        .by_name(file_name)
        .map_err(|_| SdeArchiveError::MissingRequiredFile(file_name))?;
    let reader = BufReader::new(file);
    reader
        .lines()
        .collect::<Result<Vec<_>, _>>()
        .map_err(SdeArchiveError::Open)
}

fn parse_lines<T>(
    file_name: &'static str,
    lines: Vec<String>,
    parse: fn(&str) -> Result<T, crate::SdeParseError>,
) -> Result<Vec<T>, SdeArchiveError> {
    lines
        .into_iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            parse(&line).map_err(|source| SdeArchiveError::ParseLine {
                file_name,
                line_number: index + 1,
                source,
            })
        })
        .collect()
}

pub fn read_catalog_archive(path: impl AsRef<Path>) -> Result<CatalogArchive, SdeArchiveError> {
    let file = File::open(path)?;
    let mut zip = ZipArchive::new(file)?;

    let metadata_lines = read_required_lines(&mut zip, "_sde.jsonl")?;
    let metadata = metadata_lines
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            parse_sde_metadata_line(&line).map_err(|source| SdeArchiveError::ParseLine {
                file_name: "_sde.jsonl",
                line_number: 1,
                source,
            })
        })
        .transpose()?
        .unwrap_or(SdeMetadata {
            build_number: None,
            release_date: None,
        });

    let types = parse_lines(
        "types.jsonl",
        read_required_lines(&mut zip, "types.jsonl")?,
        parse_type_line,
    )?;
    let groups = parse_lines(
        "groups.jsonl",
        read_required_lines(&mut zip, "groups.jsonl")?,
        parse_group_line,
    )?;
    let categories = parse_lines(
        "categories.jsonl",
        read_required_lines(&mut zip, "categories.jsonl")?,
        parse_category_line,
    )?;
    let market_groups = parse_lines(
        "marketGroups.jsonl",
        read_required_lines(&mut zip, "marketGroups.jsonl")?,
        parse_market_group_line,
    )?;

    Ok(CatalogArchive {
        metadata,
        types,
        groups,
        categories,
        market_groups,
    })
}
```

- [ ] **Step 5: Export archive API**

Modify `crates/sde/src/lib.rs`:

```rust
pub mod archive;
pub mod models;
pub mod parser;

pub use archive::{read_catalog_archive, CatalogArchive, SdeArchiveError};
pub use models::{CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata};
pub use parser::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, SdeParseError,
};
```

- [ ] **Step 6: Run SDE tests**

Run:

```bash
cargo test -p evetools-sde
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/sde/src/archive.rs crates/sde/src/lib.rs crates/sde/tests/fixtures.rs crates/sde/tests/parser.rs
git commit -m "feat: read sde catalog archives"
```

## Task 4: Add SQLite Catalog Schema

**Files:**

- Modify: `crates/db/Cargo.toml`
- Replace: `crates/db/src/lib.rs`
- Create: `crates/db/src/schema.rs`
- Create: `crates/db/tests/catalog_repository.rs`

- [ ] **Step 1: Add database dependencies**

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
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 2: Write schema migration tests**

Create `crates/db/tests/catalog_repository.rs`:

```rust
use evetools_db::{initialize_catalog_schema, open_memory_connection};

#[test]
fn initializes_catalog_schema() {
    let connection = open_memory_connection().unwrap();
    initialize_catalog_schema(&connection).unwrap();

    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN (
                'sde_imports',
                'inventory_types',
                'inventory_groups',
                'inventory_categories',
                'market_groups'
            )",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(table_count, 5);
}

#[test]
fn storage_mode_reports_sqlite_catalog() {
    assert_eq!(evetools_db::storage_mode(), "sqlite-catalog");
}
```

- [ ] **Step 3: Run schema tests and verify they fail**

Run:

```bash
cargo test -p evetools-db initializes_catalog_schema storage_mode_reports_sqlite_catalog
```

Expected: FAIL with unresolved functions and old `storage_mode` value.

- [ ] **Step 4: Implement schema module**

Create `crates/db/src/schema.rs`:

```rust
use rusqlite::Connection;

pub fn open_memory_connection() -> rusqlite::Result<Connection> {
    Connection::open_in_memory()
}

pub fn initialize_catalog_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS sde_imports (
            import_id INTEGER PRIMARY KEY AUTOINCREMENT,
            build_number INTEGER,
            release_date TEXT,
            source_url TEXT NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            status TEXT NOT NULL,
            error_summary TEXT,
            type_count INTEGER NOT NULL DEFAULT 0,
            group_count INTEGER NOT NULL DEFAULT 0,
            category_count INTEGER NOT NULL DEFAULT 0,
            market_group_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS inventory_types (
            type_id INTEGER PRIMARY KEY,
            group_id INTEGER NOT NULL,
            market_group_id INTEGER,
            published INTEGER NOT NULL,
            volume REAL,
            packaged_volume REAL,
            capacity REAL,
            mass REAL,
            portion_size INTEGER,
            meta_level INTEGER,
            name_en TEXT,
            name_zh TEXT,
            description_en TEXT,
            description_zh TEXT,
            raw_name_json TEXT NOT NULL,
            raw_description_json TEXT,
            updated_import_id INTEGER NOT NULL,
            FOREIGN KEY(updated_import_id) REFERENCES sde_imports(import_id)
        );

        CREATE TABLE IF NOT EXISTS inventory_groups (
            group_id INTEGER PRIMARY KEY,
            category_id INTEGER NOT NULL,
            published INTEGER NOT NULL,
            name_en TEXT,
            name_zh TEXT,
            raw_name_json TEXT NOT NULL,
            updated_import_id INTEGER NOT NULL,
            FOREIGN KEY(updated_import_id) REFERENCES sde_imports(import_id)
        );

        CREATE TABLE IF NOT EXISTS inventory_categories (
            category_id INTEGER PRIMARY KEY,
            published INTEGER NOT NULL,
            name_en TEXT,
            name_zh TEXT,
            raw_name_json TEXT NOT NULL,
            updated_import_id INTEGER NOT NULL,
            FOREIGN KEY(updated_import_id) REFERENCES sde_imports(import_id)
        );

        CREATE TABLE IF NOT EXISTS market_groups (
            market_group_id INTEGER PRIMARY KEY,
            parent_group_id INTEGER,
            name_en TEXT,
            name_zh TEXT,
            description_en TEXT,
            description_zh TEXT,
            raw_name_json TEXT NOT NULL,
            raw_description_json TEXT,
            updated_import_id INTEGER NOT NULL,
            FOREIGN KEY(updated_import_id) REFERENCES sde_imports(import_id)
        );

        CREATE INDEX IF NOT EXISTS idx_inventory_types_group_id
            ON inventory_types(group_id);
        CREATE INDEX IF NOT EXISTS idx_inventory_types_market_group_id
            ON inventory_types(market_group_id);
        CREATE INDEX IF NOT EXISTS idx_inventory_types_published
            ON inventory_types(published);
        CREATE INDEX IF NOT EXISTS idx_inventory_types_name_en
            ON inventory_types(name_en);
        CREATE INDEX IF NOT EXISTS idx_inventory_types_name_zh
            ON inventory_types(name_zh);
        CREATE INDEX IF NOT EXISTS idx_inventory_groups_category_id
            ON inventory_groups(category_id);
        CREATE INDEX IF NOT EXISTS idx_market_groups_parent_group_id
            ON market_groups(parent_group_id);
        "#,
    )
}
```

- [ ] **Step 5: Export schema and update storage mode**

Replace `crates/db/src/lib.rs`:

```rust
pub mod schema;

use thiserror::Error;

pub use schema::{initialize_catalog_schema, open_memory_connection};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DbError {
    #[error("database is not initialized")]
    NotInitialized,
}

pub fn storage_mode() -> &'static str {
    "sqlite-catalog"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_crate_reports_sqlite_catalog_storage() {
        assert_eq!(storage_mode(), "sqlite-catalog");
        assert_eq!(
            DbError::NotInitialized.to_string(),
            "database is not initialized"
        );
    }
}
```

- [ ] **Step 6: Run DB tests**

Run:

```bash
cargo test -p evetools-db
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/db/Cargo.toml crates/db/src/lib.rs crates/db/src/schema.rs crates/db/tests/catalog_repository.rs Cargo.lock
git commit -m "feat: add catalog sqlite schema"
```

## Task 5: Import Catalog Archives Transactionally

**Files:**

- Modify: `crates/db/Cargo.toml`
- Modify: `crates/db/src/lib.rs`
- Create: `crates/db/src/catalog.rs`
- Modify: `crates/db/tests/catalog_repository.rs`

- [ ] **Step 1: Depend on SDE crate**

Modify `crates/db/Cargo.toml` dependencies:

```toml
[dependencies]
chrono.workspace = true
evetools-sde = { path = "../sde" }
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Write transactional import tests**

Append to `crates/db/tests/catalog_repository.rs`:

```rust
use evetools_db::{CatalogRepository, ImportCatalogInput};
use evetools_sde::{
    CatalogArchive, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata,
};

fn sample_archive() -> CatalogArchive {
    CatalogArchive {
        metadata: SdeMetadata {
            build_number: Some(3_351_823),
            release_date: Some("2026-05-19T12:12:31Z".to_string()),
        },
        types: vec![
            CatalogType {
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
                description_en: Some("Primary building block".to_string()),
                description_zh: Some("主要建造材料".to_string()),
                raw_name_json: r#"{"en":"Tritanium","zh":"三钛合金"}"#.to_string(),
                raw_description_json: Some(
                    r#"{"en":"Primary building block","zh":"主要建造材料"}"#.to_string(),
                ),
            },
            CatalogType {
                type_id: 999_999,
                group_id: 18,
                market_group_id: None,
                published: true,
                volume: None,
                packaged_volume: None,
                capacity: None,
                mass: None,
                portion_size: Some(1),
                meta_level: None,
                name_en: Some("Unmarketed Test Item".to_string()),
                name_zh: None,
                description_en: None,
                description_zh: None,
                raw_name_json: r#"{"en":"Unmarketed Test Item"}"#.to_string(),
                raw_description_json: None,
            },
        ],
        groups: vec![CatalogGroup {
            group_id: 18,
            category_id: 4,
            published: true,
            name_en: Some("Mineral".to_string()),
            name_zh: Some("矿物".to_string()),
            raw_name_json: r#"{"en":"Mineral","zh":"矿物"}"#.to_string(),
        }],
        categories: vec![CatalogCategory {
            category_id: 4,
            published: true,
            name_en: Some("Material".to_string()),
            name_zh: Some("材料".to_string()),
            raw_name_json: r#"{"en":"Material","zh":"材料"}"#.to_string(),
        }],
        market_groups: vec![CatalogMarketGroup {
            market_group_id: 1857,
            parent_group_id: Some(1031),
            name_en: Some("Minerals".to_string()),
            name_zh: Some("矿物".to_string()),
            description_en: Some("Raw materials".to_string()),
            description_zh: None,
            raw_name_json: r#"{"en":"Minerals","zh":"矿物"}"#.to_string(),
            raw_description_json: Some(r#"{"en":"Raw materials"}"#.to_string()),
        }],
    }
}

#[test]
fn imports_catalog_archive_and_records_status() {
    let mut connection = open_memory_connection().unwrap();
    initialize_catalog_schema(&connection).unwrap();
    let mut repository = CatalogRepository::new(&mut connection);

    let status = repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "file:///tmp/test-sde.zip",
        })
        .unwrap();

    assert_eq!(status.status, "success");
    assert_eq!(status.build_number, Some(3_351_823));
    assert_eq!(status.type_count, 2);
    assert_eq!(status.group_count, 1);
    assert_eq!(status.category_count, 1);
    assert_eq!(status.market_group_count, 1);
}

#[test]
fn failed_import_rolls_back_existing_catalog() {
    let mut connection = open_memory_connection().unwrap();
    initialize_catalog_schema(&connection).unwrap();
    let mut repository = CatalogRepository::new(&mut connection);

    repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "file:///tmp/test-sde.zip",
        })
        .unwrap();

    let mut broken = sample_archive();
    broken.types[0].raw_name_json = "{not-json".to_string();

    let error = repository
        .import_archive(ImportCatalogInput {
            archive: &broken,
            source_url: "file:///tmp/broken-sde.zip",
        })
        .unwrap_err();

    assert!(error.to_string().contains("invalid raw name json"));
    let tritanium_name: String = connection
        .query_row(
            "SELECT name_en FROM inventory_types WHERE type_id = 34",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tritanium_name, "Tritanium");
}
```

- [ ] **Step 3: Run import tests and verify they fail**

Run:

```bash
cargo test -p evetools-db imports_catalog_archive_and_records_status failed_import_rolls_back_existing_catalog
```

Expected: FAIL with unresolved `CatalogRepository` and `ImportCatalogInput`.

- [ ] **Step 4: Implement catalog repository import**

Create `crates/db/src/catalog.rs`:

```rust
use chrono::Utc;
use evetools_sde::{CatalogArchive, CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogDbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid raw name json for {entity} {id}: {source}")]
    InvalidRawNameJson {
        entity: &'static str,
        id: i32,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid raw description json for {entity} {id}: {source}")]
    InvalidRawDescriptionJson {
        entity: &'static str,
        id: i32,
        #[source]
        source: serde_json::Error,
    },
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

pub struct ImportCatalogInput<'a> {
    pub archive: &'a CatalogArchive,
    pub source_url: &'a str,
}

pub struct CatalogRepository<'a> {
    connection: &'a mut Connection,
}

impl<'a> CatalogRepository<'a> {
    pub fn new(connection: &'a mut Connection) -> Self {
        Self { connection }
    }

    pub fn import_archive(
        &mut self,
        input: ImportCatalogInput<'_>,
    ) -> Result<CatalogStatus, CatalogDbError> {
        validate_archive(input.archive)?;
        let now = Utc::now().to_rfc3339();
        let tx = self.connection.transaction()?;

        tx.execute(
            "INSERT INTO sde_imports (
                build_number,
                release_date,
                source_url,
                started_at,
                status
            ) VALUES (?1, ?2, ?3, ?4, 'running')",
            params![
                input.archive.metadata.build_number,
                input.archive.metadata.release_date,
                input.source_url,
                now
            ],
        )?;
        let import_id = tx.last_insert_rowid();

        replace_categories(&tx, import_id, &input.archive.categories)?;
        replace_groups(&tx, import_id, &input.archive.groups)?;
        replace_market_groups(&tx, import_id, &input.archive.market_groups)?;
        replace_types(&tx, import_id, &input.archive.types)?;

        let completed_at = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE sde_imports
             SET completed_at = ?1,
                 status = 'success',
                 type_count = ?2,
                 group_count = ?3,
                 category_count = ?4,
                 market_group_count = ?5
             WHERE import_id = ?6",
            params![
                completed_at,
                input.archive.types.len() as i64,
                input.archive.groups.len() as i64,
                input.archive.categories.len() as i64,
                input.archive.market_groups.len() as i64,
                import_id
            ],
        )?;
        tx.commit()?;

        self.latest_status()
    }

    pub fn latest_status(&self) -> Result<CatalogStatus, CatalogDbError> {
        let status = self
            .connection
            .query_row(
                "SELECT
                    status,
                    build_number,
                    release_date,
                    source_url,
                    completed_at,
                    error_summary,
                    type_count,
                    group_count,
                    category_count,
                    market_group_count
                 FROM sde_imports
                 ORDER BY import_id DESC
                 LIMIT 1",
                [],
                |row| {
                    Ok(CatalogStatus {
                        status: row.get(0)?,
                        build_number: row.get(1)?,
                        release_date: row.get(2)?,
                        source_url: row.get(3)?,
                        completed_at: row.get(4)?,
                        error_summary: row.get(5)?,
                        type_count: row.get(6)?,
                        group_count: row.get(7)?,
                        category_count: row.get(8)?,
                        market_group_count: row.get(9)?,
                    })
                },
            )
            .optional()?;

        Ok(status.unwrap_or(CatalogStatus {
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
}

fn validate_archive(archive: &CatalogArchive) -> Result<(), CatalogDbError> {
    for row in &archive.types {
        validate_json("inventory type", row.type_id, &row.raw_name_json)?;
        validate_optional_json(
            "inventory type",
            row.type_id,
            row.raw_description_json.as_deref(),
        )?;
    }
    for row in &archive.groups {
        validate_json("inventory group", row.group_id, &row.raw_name_json)?;
    }
    for row in &archive.categories {
        validate_json("inventory category", row.category_id, &row.raw_name_json)?;
    }
    for row in &archive.market_groups {
        validate_json("market group", row.market_group_id, &row.raw_name_json)?;
        validate_optional_json(
            "market group",
            row.market_group_id,
            row.raw_description_json.as_deref(),
        )?;
    }
    Ok(())
}

fn validate_json(entity: &'static str, id: i32, value: &str) -> Result<(), CatalogDbError> {
    serde_json::from_str::<serde_json::Value>(value).map(|_| ()).map_err(|source| {
        CatalogDbError::InvalidRawNameJson { entity, id, source }
    })
}

fn validate_optional_json(
    entity: &'static str,
    id: i32,
    value: Option<&str>,
) -> Result<(), CatalogDbError> {
    if let Some(value) = value {
        serde_json::from_str::<serde_json::Value>(value).map(|_| ()).map_err(|source| {
            CatalogDbError::InvalidRawDescriptionJson { entity, id, source }
        })?;
    }
    Ok(())
}

fn replace_types(
    tx: &Transaction<'_>,
    import_id: i64,
    rows: &[CatalogType],
) -> Result<(), CatalogDbError> {
    for row in rows {
        tx.execute(
            "INSERT OR REPLACE INTO inventory_types (
                type_id,
                group_id,
                market_group_id,
                published,
                volume,
                packaged_volume,
                capacity,
                mass,
                portion_size,
                meta_level,
                name_en,
                name_zh,
                description_en,
                description_zh,
                raw_name_json,
                raw_description_json,
                updated_import_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                row.type_id,
                row.group_id,
                row.market_group_id,
                row.published,
                row.volume,
                row.packaged_volume,
                row.capacity,
                row.mass,
                row.portion_size,
                row.meta_level,
                row.name_en.as_deref(),
                row.name_zh.as_deref(),
                row.description_en.as_deref(),
                row.description_zh.as_deref(),
                row.raw_name_json.as_str(),
                row.raw_description_json.as_deref(),
                import_id
            ],
        )?;
    }
    Ok(())
}

fn replace_groups(
    tx: &Transaction<'_>,
    import_id: i64,
    rows: &[CatalogGroup],
) -> Result<(), CatalogDbError> {
    for row in rows {
        tx.execute(
            "INSERT OR REPLACE INTO inventory_groups (
                group_id,
                category_id,
                published,
                name_en,
                name_zh,
                raw_name_json,
                updated_import_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.group_id,
                row.category_id,
                row.published,
                row.name_en.as_deref(),
                row.name_zh.as_deref(),
                row.raw_name_json.as_str(),
                import_id
            ],
        )?;
    }
    Ok(())
}

fn replace_categories(
    tx: &Transaction<'_>,
    import_id: i64,
    rows: &[CatalogCategory],
) -> Result<(), CatalogDbError> {
    for row in rows {
        tx.execute(
            "INSERT OR REPLACE INTO inventory_categories (
                category_id,
                published,
                name_en,
                name_zh,
                raw_name_json,
                updated_import_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.category_id,
                row.published,
                row.name_en.as_deref(),
                row.name_zh.as_deref(),
                row.raw_name_json.as_str(),
                import_id
            ],
        )?;
    }
    Ok(())
}

fn replace_market_groups(
    tx: &Transaction<'_>,
    import_id: i64,
    rows: &[CatalogMarketGroup],
) -> Result<(), CatalogDbError> {
    for row in rows {
        tx.execute(
            "INSERT OR REPLACE INTO market_groups (
                market_group_id,
                parent_group_id,
                name_en,
                name_zh,
                description_en,
                description_zh,
                raw_name_json,
                raw_description_json,
                updated_import_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.market_group_id,
                row.parent_group_id,
                row.name_en.as_deref(),
                row.name_zh.as_deref(),
                row.description_en.as_deref(),
                row.description_zh.as_deref(),
                row.raw_name_json.as_str(),
                row.raw_description_json.as_deref(),
                import_id
            ],
        )?;
    }
    Ok(())
}
```

- [ ] **Step 5: Export catalog repository**

Modify `crates/db/src/lib.rs`:

```rust
pub mod catalog;
pub mod schema;

use thiserror::Error;

pub use catalog::{CatalogDbError, CatalogRepository, CatalogStatus, ImportCatalogInput};
pub use schema::{initialize_catalog_schema, open_memory_connection};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DbError {
    #[error("database is not initialized")]
    NotInitialized,
}

pub fn storage_mode() -> &'static str {
    "sqlite-catalog"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_crate_reports_sqlite_catalog_storage() {
        assert_eq!(storage_mode(), "sqlite-catalog");
        assert_eq!(
            DbError::NotInitialized.to_string(),
            "database is not initialized"
        );
    }
}
```

- [ ] **Step 6: Run import tests**

Run:

```bash
cargo test -p evetools-db imports_catalog_archive_and_records_status failed_import_rolls_back_existing_catalog
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/db/Cargo.toml crates/db/src/catalog.rs crates/db/src/lib.rs crates/db/tests/catalog_repository.rs Cargo.lock
git commit -m "feat: import sde catalog into sqlite"
```

## Task 6: Add Catalog Lookup, Search, And Eligibility Queries

**Files:**

- Modify: `crates/db/src/catalog.rs`
- Modify: `crates/db/src/lib.rs`
- Modify: `crates/db/tests/catalog_repository.rs`

- [ ] **Step 1: Write repository query tests**

Append to `crates/db/tests/catalog_repository.rs`:

```rust
#[test]
fn looks_up_type_with_localized_display_name() {
    let mut connection = open_memory_connection().unwrap();
    initialize_catalog_schema(&connection).unwrap();
    let mut repository = CatalogRepository::new(&mut connection);
    repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "file:///tmp/test-sde.zip",
        })
        .unwrap();

    let zh = repository.get_inventory_type(34, "zh").unwrap().unwrap();
    let en = repository.get_inventory_type(34, "en").unwrap().unwrap();

    assert_eq!(zh.display_name, "三钛合金");
    assert_eq!(en.display_name, "Tritanium");
    assert_eq!(zh.group_name, Some("矿物".to_string()));
    assert_eq!(zh.category_name, Some("材料".to_string()));
    assert_eq!(zh.market_group_name, Some("矿物".to_string()));
    assert!(zh.market_eligible);
}

#[test]
fn searches_by_english_and_chinese_names() {
    let mut connection = open_memory_connection().unwrap();
    initialize_catalog_schema(&connection).unwrap();
    let mut repository = CatalogRepository::new(&mut connection);
    repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "file:///tmp/test-sde.zip",
        })
        .unwrap();

    let english = repository.search_inventory_types("Tri", "en", 10).unwrap();
    let chinese = repository.search_inventory_types("三钛", "zh", 10).unwrap();

    assert_eq!(english[0].type_id, 34);
    assert_eq!(english[0].display_name, "Tritanium");
    assert_eq!(chinese[0].type_id, 34);
    assert_eq!(chinese[0].display_name, "三钛合金");
}

#[test]
fn lists_market_eligible_types_only() {
    let mut connection = open_memory_connection().unwrap();
    initialize_catalog_schema(&connection).unwrap();
    let mut repository = CatalogRepository::new(&mut connection);
    repository
        .import_archive(ImportCatalogInput {
            archive: &sample_archive(),
            source_url: "file:///tmp/test-sde.zip",
        })
        .unwrap();

    let rows = repository.market_eligible_types(100).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].type_id, 34);
}
```

- [ ] **Step 2: Run query tests and verify they fail**

Run:

```bash
cargo test -p evetools-db looks_up_type_with_localized_display_name searches_by_english_and_chinese_names lists_market_eligible_types_only
```

Expected: FAIL with unresolved query methods.

- [ ] **Step 3: Add view structs and query methods**

Append to `crates/db/src/catalog.rs` after `ImportCatalogInput`:

```rust
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
```

Append these methods inside `impl<'a> CatalogRepository<'a>`:

```rust
    pub fn get_inventory_type(
        &self,
        type_id: i32,
        language: &str,
    ) -> Result<Option<InventoryTypeView>, CatalogDbError> {
        let sql = inventory_type_select_sql("WHERE t.type_id = ?1");
        self.connection
            .query_row(
                sql.as_str(),
                params![type_id],
                |row| inventory_type_from_row(row, language),
            )
            .optional()
            .map_err(CatalogDbError::from)
    }

    pub fn search_inventory_types(
        &self,
        query: &str,
        language: &str,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, CatalogDbError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("%{}%", trimmed);
        let sql = inventory_type_select_sql(
            "WHERE t.name_en LIKE ?1 OR t.name_zh LIKE ?1
             ORDER BY
                CASE
                    WHEN t.name_en = ?2 OR t.name_zh = ?2 THEN 0
                    WHEN t.name_en LIKE ?3 OR t.name_zh LIKE ?3 THEN 1
                    ELSE 2
                END,
                t.name_en COLLATE NOCASE
             LIMIT ?4",
        );
        let mut statement = self.connection.prepare(sql.as_str())?;
        let prefix = format!("{}%", trimmed);
        let rows = statement.query_map(params![pattern, trimmed, prefix, limit], |row| {
            inventory_type_from_row(row, language)
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(CatalogDbError::from)
    }

    pub fn market_eligible_types(
        &self,
        limit: i64,
    ) -> Result<Vec<InventoryTypeView>, CatalogDbError> {
        let sql = inventory_type_select_sql(
            "WHERE t.published = 1
               AND t.market_group_id IS NOT NULL
               AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)
             ORDER BY t.type_id
             LIMIT ?1",
        );
        let mut statement = self.connection.prepare(sql.as_str())?;
        let rows = statement.query_map(params![limit], |row| inventory_type_from_row(row, "en"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(CatalogDbError::from)
    }
```

Append helper functions near the bottom of `crates/db/src/catalog.rs`:

```rust
fn inventory_type_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT
            t.type_id,
            t.group_id,
            g.category_id,
            t.market_group_id,
            t.name_en,
            t.name_zh,
            g.name_en,
            g.name_zh,
            c.name_en,
            c.name_zh,
            mg.name_en,
            mg.name_zh,
            t.published,
            CASE
                WHEN t.published = 1
                 AND t.market_group_id IS NOT NULL
                 AND (t.name_en IS NOT NULL OR t.name_zh IS NOT NULL)
                THEN 1
                ELSE 0
            END
         FROM inventory_types t
         LEFT JOIN inventory_groups g ON g.group_id = t.group_id
         LEFT JOIN inventory_categories c ON c.category_id = g.category_id
         LEFT JOIN market_groups mg ON mg.market_group_id = t.market_group_id
         {}",
        where_clause
    )
}

fn display_name(language: &str, zh: Option<&str>, en: Option<&str>, fallback: String) -> String {
    if language.starts_with("zh") {
        zh.or(en).map(str::to_string).unwrap_or(fallback)
    } else {
        en.or(zh).map(str::to_string).unwrap_or(fallback)
    }
}

fn inventory_type_from_row(
    row: &rusqlite::Row<'_>,
    language: &str,
) -> rusqlite::Result<InventoryTypeView> {
    let type_id: i32 = row.get(0)?;
    let name_en: Option<String> = row.get(4)?;
    let name_zh: Option<String> = row.get(5)?;
    let group_name_en: Option<String> = row.get(6)?;
    let group_name_zh: Option<String> = row.get(7)?;
    let category_name_en: Option<String> = row.get(8)?;
    let category_name_zh: Option<String> = row.get(9)?;
    let market_group_name_en: Option<String> = row.get(10)?;
    let market_group_name_zh: Option<String> = row.get(11)?;
    let published: bool = row.get(12)?;
    let market_eligible: bool = row.get(13)?;

    Ok(InventoryTypeView {
        type_id,
        group_id: row.get(1)?,
        category_id: row.get(2)?,
        market_group_id: row.get(3)?,
        display_name: display_name(
            language,
            name_zh.as_deref(),
            name_en.as_deref(),
            format!("Type {}", type_id),
        ),
        name_en,
        name_zh,
        group_name: Some(display_name(
            language,
            group_name_zh.as_deref(),
            group_name_en.as_deref(),
            String::new(),
        ))
        .filter(|value| !value.is_empty()),
        category_name: Some(display_name(
            language,
            category_name_zh.as_deref(),
            category_name_en.as_deref(),
            String::new(),
        ))
        .filter(|value| !value.is_empty()),
        market_group_name: Some(display_name(
            language,
            market_group_name_zh.as_deref(),
            market_group_name_en.as_deref(),
            String::new(),
        ))
        .filter(|value| !value.is_empty()),
        published,
        market_eligible,
    })
}
```

- [ ] **Step 4: Export `InventoryTypeView`**

Modify the export line in `crates/db/src/lib.rs`:

```rust
pub use catalog::{
    CatalogDbError, CatalogRepository, CatalogStatus, ImportCatalogInput, InventoryTypeView,
};
```

- [ ] **Step 5: Run query tests**

Run:

```bash
cargo test -p evetools-db looks_up_type_with_localized_display_name searches_by_english_and_chinese_names lists_market_eligible_types_only
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/db/src/catalog.rs crates/db/src/lib.rs crates/db/tests/catalog_repository.rs
git commit -m "feat: query static catalog"
```

## Task 7: Add Official SDE Latest Download Client

**Files:**

- Modify: `crates/sde/src/lib.rs`
- Create: `crates/sde/src/client.rs`
- Create: `crates/sde/tests/client.rs`

- [ ] **Step 1: Write SDE client tests**

Create `crates/sde/tests/client.rs`:

```rust
use evetools_sde::{SdeClient, SdeLatestMetadata};
use httpmock::prelude::*;

#[tokio::test]
async fn fetches_latest_metadata() {
    let server = MockServer::start_async().await;
    let latest = server
        .mock_async(|when, then| {
            when.method(GET).path("/static-data/tranquility/latest.jsonl");
            then.status(200)
                .header("content-type", "application/jsonl")
                .body(r#"{"_key":"sde","buildNumber":3351823,"releaseDate":"2026-05-19T12:12:31Z"}"#);
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let metadata: SdeLatestMetadata = client.latest_metadata().await.unwrap();

    latest.assert_async().await;
    assert_eq!(metadata.build_number, 3_351_823);
    assert_eq!(metadata.release_date, "2026-05-19T12:12:31Z");
}

#[tokio::test]
async fn downloads_latest_archive_bytes() {
    let server = MockServer::start_async().await;
    let archive = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/static-data/eve-online-static-data-latest-jsonl.zip");
            then.status(200).body("zip-bytes");
        })
        .await;

    let client = SdeClient::new(server.base_url()).unwrap();
    let bytes = client.download_latest_archive().await.unwrap();

    archive.assert_async().await;
    assert_eq!(bytes.as_ref(), b"zip-bytes");
}
```

- [ ] **Step 2: Add dev dependency for HTTP mocks**

Modify `crates/sde/Cargo.toml`:

```toml
[dev-dependencies]
httpmock = "0.8.3"
tempfile.workspace = true
tokio.workspace = true
```

- [ ] **Step 3: Run SDE client tests and verify they fail**

Run:

```bash
cargo test -p evetools-sde --test client
```

Expected: FAIL with unresolved `SdeClient` and `SdeLatestMetadata`.

- [ ] **Step 4: Implement SDE client**

Create `crates/sde/src/client.rs`:

```rust
use reqwest::Url;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const LATEST_METADATA_PATH: &str = "/static-data/tranquility/latest.jsonl";
const LATEST_ARCHIVE_PATH: &str = "/static-data/eve-online-static-data-latest-jsonl.zip";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    #[error("invalid SDE base URL: {0}")]
    InvalidBaseUrl(#[from] url::ParseError),
    #[error("SDE HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to decode SDE metadata: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("SDE metadata response was empty")]
    EmptyMetadata,
}

#[derive(Clone, Debug)]
pub struct SdeClient {
    base_url: Url,
    client: reqwest::Client,
}

impl SdeClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, SdeClientError> {
        Ok(Self {
            base_url: Url::parse(base_url.as_ref())?,
            client: reqwest::Client::builder()
                .user_agent("EveTools static catalog importer")
                .build()?,
        })
    }

    pub fn official() -> Result<Self, SdeClientError> {
        Self::new("https://developers.eveonline.com")
    }

    pub async fn latest_metadata(&self) -> Result<SdeLatestMetadata, SdeClientError> {
        let url = self.base_url.join(LATEST_METADATA_PATH)?;
        let body = self.client.get(url).send().await?.error_for_status()?.text().await?;
        let line = body.lines().find(|line| !line.trim().is_empty()).ok_or(
            SdeClientError::EmptyMetadata,
        )?;
        let raw: RawLatestMetadata = serde_json::from_str(line)?;

        Ok(SdeLatestMetadata {
            build_number: raw.build_number,
            release_date: raw.release_date,
        })
    }

    pub async fn download_latest_archive(&self) -> Result<bytes::Bytes, SdeClientError> {
        let url = self.base_url.join(LATEST_ARCHIVE_PATH)?;
        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?)
    }
}
```

- [ ] **Step 5: Add explicit dependencies needed by client**

Modify `crates/sde/Cargo.toml` dependencies:

```toml
[dependencies]
bytes = "1"
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
url = "2"
zip.workspace = true
```

- [ ] **Step 6: Export client API**

Modify `crates/sde/src/lib.rs`:

```rust
pub mod archive;
pub mod client;
pub mod models;
pub mod parser;

pub use archive::{read_catalog_archive, CatalogArchive, SdeArchiveError};
pub use client::{SdeClient, SdeClientError, SdeLatestMetadata};
pub use models::{CatalogCategory, CatalogGroup, CatalogMarketGroup, CatalogType, SdeMetadata};
pub use parser::{
    parse_category_line, parse_group_line, parse_market_group_line, parse_sde_metadata_line,
    parse_type_line, SdeParseError,
};
```

- [ ] **Step 7: Run SDE client tests**

Run:

```bash
cargo test -p evetools-sde --test client
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/sde/Cargo.toml crates/sde/src/client.rs crates/sde/src/lib.rs crates/sde/tests/client.rs Cargo.lock
git commit -m "feat: add sde download client"
```

## Task 8: Wire Catalog Services Into Desktop Commands

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/commands.ts`

- [ ] **Step 1: Write desktop command tests**

Append to `apps/desktop/src-tauri/src/lib.rs` test module:

```rust
    #[test]
    fn catalog_status_reports_not_imported_before_import() {
        let status = get_sde_catalog_status_with_path(":memory:".to_string()).unwrap();

        assert_eq!(status.status, "not-imported");
        assert_eq!(status.type_count, 0);
    }
```

- [ ] **Step 2: Run desktop command test and verify it fails**

Run:

```bash
cargo test -p evetools-desktop catalog_status_reports_not_imported_before_import
```

Expected: FAIL with unresolved `get_sde_catalog_status_with_path`.

- [ ] **Step 3: Add desktop dependencies**

Modify `apps/desktop/src-tauri/Cargo.toml` dependencies:

```toml
[dependencies]
chrono.workspace = true
evetools-db = { path = "../../../crates/db" }
evetools-domain = { path = "../../../crates/domain" }
evetools-esi = { path = "../../../crates/esi" }
evetools-sde = { path = "../../../crates/sde" }
evetools-worker = { path = "../../../crates/worker" }
rust_decimal.workspace = true
serde.workspace = true
serde_json.workspace = true
tauri.workspace = true
tokio.workspace = true
```

- [ ] **Step 4: Add catalog command imports**

At the top of `apps/desktop/src-tauri/src/lib.rs`, add:

```rust
use evetools_db::{
    initialize_catalog_schema, CatalogRepository, CatalogStatus, ImportCatalogInput,
    InventoryTypeView,
};
use evetools_sde::read_catalog_archive;
use rusqlite::Connection;
```

Also add `rusqlite.workspace = true` to `apps/desktop/src-tauri/Cargo.toml` dependencies because the helper opens connections directly.

- [ ] **Step 5: Add catalog helper and commands**

Add these functions before `pub fn run()` in `apps/desktop/src-tauri/src/lib.rs`:

```rust
fn open_catalog_connection(path: &str) -> Result<Connection, String> {
    let connection = if path == ":memory:" {
        Connection::open_in_memory()
    } else {
        Connection::open(path)
    }
    .map_err(|error| error.to_string())?;
    initialize_catalog_schema(&connection).map_err(|error| error.to_string())?;
    Ok(connection)
}

fn default_catalog_db_path() -> String {
    "evetools-catalog.sqlite3".to_string()
}

#[tauri::command]
fn get_sde_catalog_status() -> Result<CatalogStatus, String> {
    get_sde_catalog_status_with_path(default_catalog_db_path())
}

fn get_sde_catalog_status_with_path(path: String) -> Result<CatalogStatus, String> {
    let mut connection = open_catalog_connection(&path)?;
    let repository = CatalogRepository::new(&mut connection);
    repository.latest_status().map_err(|error| error.to_string())
}

#[tauri::command]
fn import_sde_catalog_from_file(path: String) -> Result<CatalogStatus, String> {
    let archive = read_catalog_archive(&path).map_err(|error| error.to_string())?;
    let mut connection = open_catalog_connection(&default_catalog_db_path())?;
    let mut repository = CatalogRepository::new(&mut connection);
    repository
        .import_archive(ImportCatalogInput {
            archive: &archive,
            source_url: &format!("file://{}", path),
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn search_inventory_types(
    query: String,
    language: String,
    limit: i64,
) -> Result<Vec<InventoryTypeView>, String> {
    let mut connection = open_catalog_connection(&default_catalog_db_path())?;
    let repository = CatalogRepository::new(&mut connection);
    repository
        .search_inventory_types(&query, &language, limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_inventory_type(type_id: i32, language: String) -> Result<Option<InventoryTypeView>, String> {
    let mut connection = open_catalog_connection(&default_catalog_db_path())?;
    let repository = CatalogRepository::new(&mut connection);
    repository
        .get_inventory_type(type_id, &language)
        .map_err(|error| error.to_string())
}
```

- [ ] **Step 6: Register catalog commands**

Modify the `tauri::generate_handler!` list:

```rust
        .invoke_handler(tauri::generate_handler![
            lookup_market_price,
            list_selection_candidates,
            list_order_monitor_items,
            get_sync_status,
            get_sde_catalog_status,
            import_sde_catalog_from_file,
            search_inventory_types,
            get_inventory_type
        ])
```

- [ ] **Step 7: Add TypeScript wrappers**

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

export function importSdeCatalogFromFile(path: string): Promise<CatalogStatus> {
  return invoke<CatalogStatus>("import_sde_catalog_from_file", { path });
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

- [ ] **Step 8: Run desktop and type checks**

Run:

```bash
cargo test -p evetools-desktop catalog_status_reports_not_imported_before_import
pnpm --filter @evetools/desktop typecheck
```

Expected: both PASS.

- [ ] **Step 9: Commit**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/lib.rs apps/desktop/src/commands.ts Cargo.lock
git commit -m "feat: expose static catalog commands"
```

## Task 9: Add Latest Import Service Function

**Files:**

- Modify: `crates/sde/src/archive.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add archive reader from bytes**

Add this test to `crates/sde/tests/parser.rs`:

```rust
#[test]
fn reads_catalog_archive_from_bytes() {
    let archive_file = NamedTempFile::new().unwrap();
    fixtures::write_test_sde_zip(archive_file.path());
    let bytes = std::fs::read(archive_file.path()).unwrap();

    let archive = evetools_sde::read_catalog_archive_from_bytes(bytes).unwrap();

    assert_eq!(archive.metadata.build_number, Some(3_351_823));
    assert_eq!(archive.types.len(), 2);
}
```

- [ ] **Step 2: Run new archive test and verify it fails**

Run:

```bash
cargo test -p evetools-sde reads_catalog_archive_from_bytes
```

Expected: FAIL with unresolved `read_catalog_archive_from_bytes`.

- [ ] **Step 3: Implement byte archive reader**

Modify `crates/sde/src/archive.rs`:

```rust
use std::io::Cursor;
```

Add:

```rust
pub fn read_catalog_archive_from_bytes(
    bytes: impl Into<Vec<u8>>,
) -> Result<CatalogArchive, SdeArchiveError> {
    let cursor = Cursor::new(bytes.into());
    let zip = ZipArchive::new(cursor)?;
    read_catalog_archive_from_zip(zip)
}
```

Refactor the body of `read_catalog_archive` so it calls a shared helper:

```rust
pub fn read_catalog_archive(path: impl AsRef<Path>) -> Result<CatalogArchive, SdeArchiveError> {
    let file = File::open(path)?;
    let zip = ZipArchive::new(file)?;
    read_catalog_archive_from_zip(zip)
}

fn read_catalog_archive_from_zip<R: Read + std::io::Seek>(
    mut zip: ZipArchive<R>,
) -> Result<CatalogArchive, SdeArchiveError> {
    let metadata_lines = read_required_lines(&mut zip, "_sde.jsonl")?;
    let metadata = metadata_lines
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            parse_sde_metadata_line(&line).map_err(|source| SdeArchiveError::ParseLine {
                file_name: "_sde.jsonl",
                line_number: 1,
                source,
            })
        })
        .transpose()?
        .unwrap_or(SdeMetadata {
            build_number: None,
            release_date: None,
        });

    let types = parse_lines(
        "types.jsonl",
        read_required_lines(&mut zip, "types.jsonl")?,
        parse_type_line,
    )?;
    let groups = parse_lines(
        "groups.jsonl",
        read_required_lines(&mut zip, "groups.jsonl")?,
        parse_group_line,
    )?;
    let categories = parse_lines(
        "categories.jsonl",
        read_required_lines(&mut zip, "categories.jsonl")?,
        parse_category_line,
    )?;
    let market_groups = parse_lines(
        "marketGroups.jsonl",
        read_required_lines(&mut zip, "marketGroups.jsonl")?,
        parse_market_group_line,
    )?;

    Ok(CatalogArchive {
        metadata,
        types,
        groups,
        categories,
        market_groups,
    })
}
```

- [ ] **Step 4: Export byte archive reader**

Modify `crates/sde/src/lib.rs` archive export:

```rust
pub use archive::{
    read_catalog_archive, read_catalog_archive_from_bytes, CatalogArchive, SdeArchiveError,
};
```

- [ ] **Step 5: Add latest import command**

Modify SDE imports in `apps/desktop/src-tauri/src/lib.rs`:

```rust
use evetools_sde::{read_catalog_archive, read_catalog_archive_from_bytes, SdeClient};
```

Add command:

```rust
#[tauri::command]
async fn import_sde_catalog_latest() -> Result<CatalogStatus, String> {
    let client = SdeClient::official().map_err(|error| error.to_string())?;
    let metadata = client.latest_metadata().await.map_err(|error| error.to_string())?;

    {
        let mut connection = open_catalog_connection(&default_catalog_db_path())?;
        let repository = CatalogRepository::new(&mut connection);
        let status = repository.latest_status().map_err(|error| error.to_string())?;
        if status.status == "success" && status.build_number == Some(metadata.build_number) {
            return Ok(status);
        }
    }

    let bytes = client
        .download_latest_archive()
        .await
        .map_err(|error| error.to_string())?;
    let archive = read_catalog_archive_from_bytes(bytes.to_vec()).map_err(|error| error.to_string())?;
    let mut connection = open_catalog_connection(&default_catalog_db_path())?;
    let mut repository = CatalogRepository::new(&mut connection);
    repository
        .import_archive(ImportCatalogInput {
            archive: &archive,
            source_url: "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip",
        })
        .map_err(|error| error.to_string())
}
```

Register it in `generate_handler!`:

```rust
            import_sde_catalog_latest,
```

- [ ] **Step 6: Add TypeScript wrapper**

Append to `apps/desktop/src/commands.ts`:

```ts
export function importSdeCatalogLatest(): Promise<CatalogStatus> {
  return invoke<CatalogStatus>("import_sde_catalog_latest");
}
```

- [ ] **Step 7: Run checks**

Run:

```bash
cargo test -p evetools-sde reads_catalog_archive_from_bytes
cargo test -p evetools-desktop
pnpm --filter @evetools/desktop typecheck
```

Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/sde/src/archive.rs crates/sde/src/lib.rs crates/sde/tests/parser.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/commands.ts Cargo.lock
git commit -m "feat: import latest sde catalog"
```

## Task 10: Document Static Catalog Import

**Files:**

- Modify: `README.md`

- [ ] **Step 1: Update README**

Add this section after `Public ESI Market Sync`:

```markdown
## Static SDE Catalog

EveTools uses CCP's official Static Data Export (SDE) as the local item catalog for search, localization, and market recommendation metadata.

The static catalog importer uses the official JSON Lines archive:

- `https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip`

The first catalog slice imports:

- `_sde.jsonl`
- `types.jsonl`
- `groups.jsonl`
- `categories.jsonl`
- `marketGroups.jsonl`

This catalog is intentionally smaller than the full SDE. Dogma, blueprints, icons, and full universe geography are deferred until a feature needs them.

The importer stores data in local SQLite and exposes Tauri commands for:

- catalog status
- local archive import
- latest official archive import
- localized item search
- type-id lookup

The catalog is a prerequisite for NPC hub selection discovery because public market orders provide `type_id` values, not localized item metadata.
```

- [ ] **Step 2: Run documentation check**

Run:

```bash
rg -n "Static SDE Catalog|types.jsonl|marketGroups.jsonl|NPC hub selection discovery" README.md
```

Expected: all four terms are found.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document static sde catalog"
```

## Task 11: Final Verification

**Files:**

- No file edits expected.

- [ ] **Step 1: Run Rust formatting check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 2: Run full Rust test suite**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Run workspace check**

Run:

```bash
pnpm check
```

Expected: PASS.

- [ ] **Step 4: Verify git status**

Run:

```bash
git status --short --branch
```

Expected: clean working tree on the feature branch, ahead by the task commits.

## Implementation Notes

- The default desktop catalog DB path in this plan is intentionally simple. A later app-storage task should move it to the Tauri app data directory.
- `SdeClientError` has a separate `Decode(#[from] serde_json::Error)` variant. Network and decode errors must be returned, not unwrapped.
- `CatalogRepository::new` takes `&mut Connection` because catalog import uses transactions.
- Archive tests use `tempfile::NamedTempFile`; do not write test archives into the repository.
- The parser currently loads each required JSONL file into a `Vec` through the archive reader. This is acceptable for the catalog MVP plan, but the public interface keeps archive reading isolated so a later optimization can stream rows directly into the repository.

## Self-Review Checklist

- Spec coverage: local archive import, latest download, SQLite schema, localized names, type lookup/search, market eligibility, transactional failure behavior, and documentation are each mapped to tasks.
- Dependency policy: the plan uses `zip`, `serde_json`, `reqwest`, `rusqlite`, and `tempfile`; no custom zip parser, JSON tokenizer, or SQL batching framework is introduced.
- Type consistency: `CatalogArchive`, `CatalogRepository`, `CatalogStatus`, `InventoryTypeView`, `SdeClient`, and command wrapper names are defined before they are used by later tasks.
- Known deferral: no full settings UI and no NPC Hub Discovery consumption are included; both are explicitly out of scope for this plan.
