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
        types: parse_lines(
            "types.jsonl",
            required_lines(&mut zip, "types.jsonl")?,
            parse_type_line,
        )?,
        groups: parse_lines(
            "groups.jsonl",
            required_lines(&mut zip, "groups.jsonl")?,
            parse_group_line,
        )?,
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
