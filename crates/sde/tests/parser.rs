use evetools_sde::{parse_type_line, read_catalog_archive_from_bytes, CatalogArchive, CatalogType};
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
