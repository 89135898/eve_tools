use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DbError {
    #[error("database is not initialized")]
    NotInitialized,
}

pub fn storage_mode() -> &'static str {
    "in-memory-fixture"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_crate_reports_fixture_storage() {
        assert_eq!(storage_mode(), "in-memory-fixture");
        assert_eq!(
            DbError::NotInitialized.to_string(),
            "database is not initialized"
        );
    }
}
