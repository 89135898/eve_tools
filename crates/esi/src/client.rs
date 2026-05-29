use crate::{
    EsiCharacterOrder, EsiError, EsiMarketHistoryDay, EsiMarketOrder, EsiOrderType,
    EsiTokenResponse, EsiTypeInfo, ResolvedInventoryType, UniverseIdsResponse,
};
use serde::de::DeserializeOwned;
use std::time::Duration;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct EsiClient {
    base_url: String,
    http: reqwest::Client,
}

impl EsiClient {
    pub fn tranquility() -> Self {
        Self::new("https://esi.evetech.net")
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeouts(base_url, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT)
    }

    pub fn with_request_timeout(base_url: impl Into<String>, request_timeout: Duration) -> Self {
        Self::with_timeouts(base_url, DEFAULT_CONNECT_TIMEOUT, request_timeout)
    }

    pub fn with_timeouts(
        base_url: impl Into<String>,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .build()
            .expect("failed to build reqwest client for ESI");
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn resolve_inventory_type(
        &self,
        query: &str,
    ) -> Result<ResolvedInventoryType, EsiError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(EsiError::ItemNotFound);
        }

        if let Ok(type_id) = trimmed.parse::<i32>() {
            let info = self.type_info(type_id).await?;
            return Ok(ResolvedInventoryType {
                type_id: info.type_id,
                name: info.name,
            });
        }

        let ids = self.universe_ids(trimmed).await?;
        let entry = ids
            .inventory_types
            .unwrap_or_default()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(trimmed))
            .ok_or(EsiError::ItemNotFound)?;

        Ok(ResolvedInventoryType {
            type_id: entry.id,
            name: entry.name,
        })
    }

    pub async fn universe_ids(&self, name: &str) -> Result<UniverseIdsResponse, EsiError> {
        let url = format!(
            "{}/latest/universe/ids/?datasource=tranquility",
            self.base_url
        );
        let request = self.http.post(url).json(&[name]);
        self.decode_response(request.send().await).await
    }

    pub async fn type_info(&self, type_id: i32) -> Result<EsiTypeInfo, EsiError> {
        let url = format!(
            "{}/latest/universe/types/{type_id}/?datasource=tranquility",
            self.base_url
        );
        let request = self.http.get(url);
        self.decode_response(request.send().await).await
    }

    pub async fn market_orders(
        &self,
        region_id: i32,
        type_id: i32,
        order_type: EsiOrderType,
    ) -> Result<Vec<EsiMarketOrder>, EsiError> {
        self.fetch_market_order_pages(region_id, Some(type_id), order_type)
            .await
    }

    pub async fn region_market_orders(
        &self,
        region_id: i32,
        order_type: EsiOrderType,
    ) -> Result<Vec<EsiMarketOrder>, EsiError> {
        self.fetch_market_order_pages(region_id, None, order_type)
            .await
    }

    async fn fetch_market_order_pages(
        &self,
        region_id: i32,
        type_id: Option<i32>,
        order_type: EsiOrderType,
    ) -> Result<Vec<EsiMarketOrder>, EsiError> {
        let mut current_page = 1;
        let mut orders = Vec::new();

        loop {
            let mut url = format!(
                "{}/latest/markets/{region_id}/orders/?datasource=tranquility&order_type={}&page={current_page}",
                self.base_url,
                order_type.as_query_value(),
            );
            if let Some(type_id) = type_id {
                url.push_str(&format!("&type_id={type_id}"));
            }
            let request = self.http.get(url);
            let response = request.send().await.map_err(EsiError::Http)?;
            let total_pages = response
                .headers()
                .get("X-Pages")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(1);
            let mut page_orders: Vec<EsiMarketOrder> = self.decode_response(Ok(response)).await?;
            orders.append(&mut page_orders);
            if current_page >= total_pages {
                break;
            }
            current_page += 1;
        }

        Ok(orders)
    }

    pub async fn market_history(
        &self,
        region_id: i32,
        type_id: i32,
    ) -> Result<Vec<EsiMarketHistoryDay>, EsiError> {
        let url = format!(
            "{}/latest/markets/{region_id}/history/?datasource=tranquility&type_id={type_id}",
            self.base_url
        );
        let request = self.http.get(url);
        self.decode_response(request.send().await).await
    }

    pub async fn exchange_authorization_code(
        &self,
        sso_base_url: &str,
        client_id: &str,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<EsiTokenResponse, EsiError> {
        let url = format!("{}/v2/oauth/token", sso_base_url.trim_end_matches('/'));
        let request = self.http.post(url).form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ]);
        self.decode_response(request.send().await).await
    }

    pub async fn refresh_access_token(
        &self,
        sso_base_url: &str,
        client_id: &str,
        refresh_token: &str,
    ) -> Result<EsiTokenResponse, EsiError> {
        let url = format!("{}/v2/oauth/token", sso_base_url.trim_end_matches('/'));
        let request = self.http.post(url).form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ]);
        self.decode_response(request.send().await).await
    }

    pub async fn character_orders(
        &self,
        character_id: i64,
        access_token: &str,
    ) -> Result<Vec<EsiCharacterOrder>, EsiError> {
        let url = format!(
            "{}/latest/characters/{character_id}/orders/?datasource=tranquility",
            self.base_url
        );
        let request = self.http.get(url).bearer_auth(access_token);
        self.decode_response(request.send().await).await
    }

    async fn decode_response<T>(
        &self,
        response_result: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<T, EsiError>
    where
        T: DeserializeOwned,
    {
        let response = response_result.map_err(EsiError::Http)?;
        let status = response.status();
        let body = response.text().await.map_err(EsiError::Http)?;

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(EsiError::ItemNotFound);
        }

        if !status.is_success() {
            return Err(EsiError::Status {
                status: status.as_u16(),
                body,
            });
        }

        serde_json::from_str::<T>(&body).map_err(EsiError::Decode)
    }
}
