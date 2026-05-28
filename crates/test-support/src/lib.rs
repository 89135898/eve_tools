use sqlx::PgPool;
use thiserror::Error;
use url::Url;

pub const TEST_DATABASE_URL_ENV: &str = "EVETOOLS_TEST_DATABASE_URL";
pub const ALLOW_REMOTE_TEST_DATABASE_ENV: &str = "EVETOOLS_TEST_DATABASE_ALLOW_REMOTE";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TestDatabaseConfigError {
    #[error("{env_var} is not a valid URL: {message}")]
    InvalidUrl {
        env_var: &'static str,
        message: String,
    },
    #[error("{env_var} must include a database host")]
    MissingHost { env_var: &'static str },
    #[error("{env_var} points at remote host {host}; use a local disposable Postgres database or set {allow_env_var}=1")]
    RemoteDatabaseUrl {
        env_var: &'static str,
        host: String,
        allow_env_var: &'static str,
    },
}

pub fn guarded_database_url_from_env() -> Result<Option<String>, TestDatabaseConfigError> {
    let database_url = std::env::var(TEST_DATABASE_URL_ENV).ok();
    validate_test_database_url(
        database_url.as_deref(),
        allow_remote_test_database_from_env(),
    )
}

pub fn validate_test_database_url(
    database_url: Option<&str>,
    allow_remote: bool,
) -> Result<Option<String>, TestDatabaseConfigError> {
    let Some(database_url) = database_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let parsed = Url::parse(database_url).map_err(|error| TestDatabaseConfigError::InvalidUrl {
        env_var: TEST_DATABASE_URL_ENV,
        message: error.to_string(),
    })?;
    let host = parsed
        .host_str()
        .ok_or(TestDatabaseConfigError::MissingHost {
            env_var: TEST_DATABASE_URL_ENV,
        })?;

    if allow_remote || is_local_database_host(host) {
        return Ok(Some(database_url.to_string()));
    }

    Err(TestDatabaseConfigError::RemoteDatabaseUrl {
        env_var: TEST_DATABASE_URL_ENV,
        host: host.to_string(),
        allow_env_var: ALLOW_REMOTE_TEST_DATABASE_ENV,
    })
}

pub async fn reset_evetools_catalog_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DROP SCHEMA IF EXISTS evetools_catalog CASCADE")
        .persistent(false)
        .execute(pool)
        .await?;
    Ok(())
}

fn allow_remote_test_database_from_env() -> bool {
    std::env::var(ALLOW_REMOTE_TEST_DATABASE_ENV)
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes")
        })
        .unwrap_or(false)
}

fn is_local_database_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_test_database_url_skips_integration_tests() {
        assert_eq!(validate_test_database_url(None, false).unwrap(), None);
    }

    #[test]
    fn local_test_database_url_is_allowed_by_default() {
        let url = "postgresql://postgres:postgres@127.0.0.1:54329/evetools_test";

        assert_eq!(
            validate_test_database_url(Some(url), false).unwrap(),
            Some(url.to_string())
        );
    }

    #[test]
    fn remote_supabase_test_database_url_is_rejected_by_default() {
        let error = validate_test_database_url(
            Some("postgresql://postgres@db.example.supabase.co:5432/postgres"),
            false,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "EVETOOLS_TEST_DATABASE_URL points at remote host db.example.supabase.co; use a local disposable Postgres database or set EVETOOLS_TEST_DATABASE_ALLOW_REMOTE=1"
        );
    }

    #[test]
    fn remote_test_database_url_can_be_explicitly_allowed() {
        let url = "postgresql://postgres@db.example.supabase.co:5432/postgres";

        assert_eq!(
            validate_test_database_url(Some(url), true).unwrap(),
            Some(url.to_string())
        );
    }
}
