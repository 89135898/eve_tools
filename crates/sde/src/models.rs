use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogLocalization {
    pub language: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

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
    pub localizations: Vec<CatalogLocalization>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogGroup {
    pub group_id: i32,
    pub category_id: i32,
    pub published: bool,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub raw_name_json: serde_json::Value,
    pub localizations: Vec<CatalogLocalization>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogCategory {
    pub category_id: i32,
    pub published: bool,
    pub name_en: Option<String>,
    pub name_zh: Option<String>,
    pub raw_name_json: serde_json::Value,
    pub localizations: Vec<CatalogLocalization>,
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
    pub localizations: Vec<CatalogLocalization>,
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
    #[serde(rename = "groupID")]
    pub group_id: i32,
    #[serde(rename = "marketGroupID")]
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
    #[serde(rename = "categoryID")]
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
    #[serde(rename = "parentGroupID")]
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
