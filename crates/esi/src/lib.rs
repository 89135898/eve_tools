use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EsiError {
    #[error("ESI client is not connected in fixture mode")]
    FixtureMode,
}

pub fn client_mode() -> &'static str {
    "fixture"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esi_crate_starts_in_fixture_mode() {
        assert_eq!(client_mode(), "fixture");
        assert_eq!(
            EsiError::FixtureMode.to_string(),
            "ESI client is not connected in fixture mode"
        );
    }
}
