use crate::models::*;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
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

fn parse_row<T: DeserializeOwned>(row_kind: &'static str, line: &str) -> Result<T, SdeParseError> {
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

fn localized_entry(map: &LocalizedMap, key: &str) -> Option<String> {
    map.get(key).filter(|value| !value.is_empty()).cloned()
}

fn localizations(
    names: &LocalizedMap,
    descriptions: Option<&LocalizedMap>,
) -> Vec<CatalogLocalization> {
    let mut languages = BTreeSet::new();
    for (language, value) in names {
        if !value.is_empty() {
            languages.insert(language.clone());
        }
    }
    if let Some(descriptions) = descriptions {
        for (language, value) in descriptions {
            if !value.is_empty() {
                languages.insert(language.clone());
            }
        }
    }

    languages
        .into_iter()
        .map(|language| CatalogLocalization {
            name: localized_entry(names, &language),
            description: descriptions.and_then(|values| localized_entry(values, &language)),
            language,
        })
        .collect()
}

pub fn parse_type_line(line: &str) -> Result<CatalogType, SdeParseError> {
    let raw: RawTypeRow = parse_row("types", line)?;
    let localizations = localizations(&raw.name, Some(&raw.description));
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
        localizations,
    })
}

pub fn parse_group_line(line: &str) -> Result<CatalogGroup, SdeParseError> {
    let raw: RawGroupRow = parse_row("groups", line)?;
    let localizations = localizations(&raw.name, None);
    Ok(CatalogGroup {
        group_id: raw.key,
        category_id: raw.category_id,
        published: raw.published,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        raw_name_json: raw_json(&raw.name),
        localizations,
    })
}

pub fn parse_category_line(line: &str) -> Result<CatalogCategory, SdeParseError> {
    let raw: RawCategoryRow = parse_row("categories", line)?;
    let localizations = localizations(&raw.name, None);
    Ok(CatalogCategory {
        category_id: raw.key,
        published: raw.published,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        raw_name_json: raw_json(&raw.name),
        localizations,
    })
}

pub fn parse_market_group_line(line: &str) -> Result<CatalogMarketGroup, SdeParseError> {
    let raw: RawMarketGroupRow = parse_row("marketGroups", line)?;
    let localizations = localizations(&raw.name, Some(&raw.description));
    Ok(CatalogMarketGroup {
        market_group_id: raw.key,
        parent_group_id: raw.parent_group_id,
        name_en: localized(&raw.name, "en"),
        name_zh: localized(&raw.name, "zh"),
        description_en: localized(&raw.description, "en"),
        description_zh: localized(&raw.description, "zh"),
        raw_name_json: raw_json(&raw.name),
        raw_description_json: optional_raw_json(&raw.description),
        localizations,
    })
}

pub fn parse_sde_metadata_line(line: &str) -> Result<SdeMetadata, SdeParseError> {
    let raw: RawSdeMetadataRow = parse_row("_sde", line)?;
    Ok(SdeMetadata {
        build_number: raw.build_number,
        release_date: raw.release_date,
    })
}
