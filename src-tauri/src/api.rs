use keyring::Entry;
use reqwest::{Client, Method, Response};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const KEYCHAIN_SERVICE: &str = "net.accly.launcher";
const KEYCHAIN_ACCOUNT: &str = "device-session";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSession {
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAccess {
    pub plan_name: String,
    pub paid: bool,
    pub suspended: bool,
    pub allowed_tiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub used: f64,
    pub daily: f64,
    pub remaining: f64,
    pub percent_used: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRecord {
    pub prefix: String,
    pub group_type: String,
    #[serde(default)]
    pub allowed_tiers: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedApiKey {
    #[serde(flatten)]
    pub record: ApiKeyRecord,
    pub full_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    pub plan: PlanAccess,
    pub usage: UsageSummary,
    pub keys: Vec<ApiKeyRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub available: bool,
    pub version: Option<String>,
    pub url: Option<String>,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

#[derive(Deserialize)]
struct DeviceTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyResponse {
    prefix: String,
    group_type: String,
    #[serde(default)]
    allowed_tiers: Vec<String>,
    created_at: String,
    full_key: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
    draft: bool,
}

struct Endpoints {
    auth: String,
    core: String,
    client_id: String,
}

impl Endpoints {
    fn load() -> Self {
        Self {
            auth: configured_value(
                "ACCLY_AUTH_URL",
                option_env!("ACCLY_AUTH_URL"),
                "https://auth.accly.net",
            ),
            core: configured_value(
                "ACCLY_CORE_URL",
                option_env!("ACCLY_CORE_URL"),
                "https://core.accly.net",
            ),
            client_id: configured_value(
                "ACCLY_LAUNCHER_CLIENT_ID",
                option_env!("ACCLY_LAUNCHER_CLIENT_ID"),
                "accly-launcher",
            ),
        }
    }

    fn auth_route(&self, route: &str) -> String {
        format!("{}{}", self.auth.trim_end_matches('/'), route)
    }

    fn core_route(&self, route: &str) -> String {
        format!("{}{}", self.core.trim_end_matches('/'), route)
    }
}

fn configured_value(name: &str, compiled: Option<&str>, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| compiled.map(ToString::to_string))
        .unwrap_or_else(|| fallback.to_string())
}

fn default_expires_in() -> u64 {
    1_800
}

fn default_interval() -> u64 {
    5
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent(format!("Accly Launcher/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("Unable to create HTTP client: {error}"))
}

fn session_entry() -> Result<Entry, String> {
    Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|error| format!("Unable to access the macOS Keychain: {error}"))
}

fn read_session_token() -> Result<Option<String>, String> {
    let entry = session_entry()?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => Ok(Some(token)),
        Ok(_) => Ok(None),
        Err(_) => Ok(None),
    }
}

fn store_session_token(token: &str) -> Result<(), String> {
    session_entry()?
        .set_password(token)
        .map_err(|error| format!("Unable to store the launcher session: {error}"))
}

pub fn get_launcher_session() -> Result<Option<LauncherSession>, String> {
    Ok(read_session_token()?.map(|_| LauncherSession { expires_at: None }))
}

pub fn clear_launcher_session() -> Result<(), String> {
    let entry = session_entry()?;
    let _ = entry.delete_credential();
    Ok(())
}

pub async fn begin_device_authorization() -> Result<DeviceCode, String> {
    let endpoints = Endpoints::load();
    let response = client()?
        .post(endpoints.auth_route("/api/auth/device/code"))
        .json(&json!({
            "client_id": endpoints.client_id,
            "scope": "launcher"
        }))
        .send()
        .await
        .map_err(|error| format!("Unable to start device authorization: {error}"))?;
    let payload: DeviceCodeResponse = response_json(response).await?;
    Ok(DeviceCode {
        device_code: payload.device_code,
        user_code: payload.user_code,
        verification_uri: payload.verification_uri,
        verification_uri_complete: payload.verification_uri_complete,
        expires_in: payload.expires_in,
        interval: payload.interval,
    })
}

pub async fn poll_device_authorization(
    device_code: DeviceCode,
) -> Result<Option<LauncherSession>, String> {
    let endpoints = Endpoints::load();
    let response = client()?
        .post(endpoints.auth_route("/api/auth/device/token"))
        .json(&json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": device_code.device_code,
            "client_id": endpoints.client_id
        }))
        .send()
        .await
        .map_err(|error| format!("Unable to complete device authorization: {error}"))?;
    let payload: DeviceTokenResponse = response_json(response).await?;
    store_session_token(&payload.access_token)?;
    Ok(Some(LauncherSession { expires_at: None }))
}

pub async fn get_account_snapshot() -> Result<AccountSnapshot, String> {
    let subscription = authorized_json(Method::GET, "/api/v1/settings/subscription", None).await?;
    let usage = authorized_json(Method::GET, "/api/v1/usage/today", None).await?;
    let keys = authorized_json(Method::GET, "/api/v1/api-keys", None).await?;

    let subscription = response_data(&subscription);
    let usage = response_data(&usage);
    let keys = response_data(&keys);
    let subscription_value = subscription.get("subscription");
    let paid = subscription_value.is_some_and(|value| !value.is_null());
    let plan_name = subscription
        .get("planName")
        .and_then(Value::as_str)
        .unwrap_or(if paid { "Paid" } else { "Free" })
        .to_string();
    let suspended = subscription
        .pointer("/access/status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "payment_suspended");
    let allowed_tiers = string_array(subscription.get("allowedTiers"));

    Ok(AccountSnapshot {
        plan: PlanAccess {
            plan_name,
            paid,
            suspended,
            allowed_tiers,
        },
        usage: UsageSummary {
            daily: number_at(usage, "daily"),
            used: number_at(usage, "used"),
            remaining: number_at(usage, "remaining"),
            percent_used: number_at(usage, "percentUsed"),
        },
        keys: parse_key_list(keys)?,
    })
}

pub async fn create_api_key(group_type: String) -> Result<CreatedApiKey, String> {
    validate_group_type(&group_type)?;
    let value = authorized_json(
        Method::POST,
        "/api/v1/api-keys",
        Some(json!({ "groupType": group_type })),
    )
    .await?;
    parse_created_key(response_data(&value))
}

pub async fn delete_api_key(prefix: String) -> Result<(), String> {
    if !prefix.starts_with("sk-") {
        return Err("The API key prefix is invalid.".to_string());
    }
    authorized_empty(Method::DELETE, &format!("/api/v1/api-keys/{prefix}"), None).await
}

pub async fn regenerate_api_key(prefix: String) -> Result<CreatedApiKey, String> {
    if !prefix.starts_with("sk-") {
        return Err("The API key prefix is invalid.".to_string());
    }
    let value = authorized_json(
        Method::POST,
        &format!("/api/v1/api-keys/{prefix}/regenerate"),
        None,
    )
    .await?;
    parse_created_key(response_data(&value))
}

pub async fn check_for_update() -> Result<UpdateStatus, String> {
    let response = client()?
        .get("https://api.github.com/repos/Accly-Labs/Accly-Launcher/releases/latest")
        .send()
        .await
        .map_err(|error| format!("Unable to check for updates: {error}"))?;
    let release: GitHubRelease = response_json(response).await?;
    if release.draft || release.prerelease {
        return Ok(UpdateStatus {
            available: false,
            version: None,
            url: None,
        });
    }

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("Invalid bundled version: {error}"))?;
    let version = release.tag_name.trim_start_matches('v');
    let available = Version::parse(version).is_ok_and(|latest| latest > current);
    Ok(UpdateStatus {
        available,
        version: available.then(|| version.to_string()),
        url: available.then_some(release.html_url),
    })
}

async fn authorized_json(
    method: Method,
    route: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let response = authorized_request(method, route, body).await?;
    response_json(response).await
}

async fn authorized_empty(method: Method, route: &str, body: Option<Value>) -> Result<(), String> {
    let response = authorized_request(method, route, body).await?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

async fn authorized_request(
    method: Method,
    route: &str,
    body: Option<Value>,
) -> Result<Response, String> {
    let token =
        read_session_token()?.ok_or_else(|| "Sign in to your Accly account first.".to_string())?;
    let endpoints = Endpoints::load();
    let request = client()?
        .request(method, endpoints.core_route(route))
        .bearer_auth(token);
    let request = match body {
        Some(body) => request.json(&body),
        None => request,
    };
    request
        .send()
        .await
        .map_err(|error| format!("Unable to contact Accly: {error}"))
}

async fn response_json<T: DeserializeOwned>(response: Response) -> Result<T, String> {
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("Accly returned an invalid response: {error}"))
}

async fn response_error(response: Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if let Ok(value) = serde_json::from_str::<Value>(&body) {
        if let Some(message) = value
            .get("error_description")
            .or_else(|| value.get("message"))
            .or_else(|| value.get("error"))
            .and_then(Value::as_str)
        {
            return message.to_string();
        }
    }
    format!("Accly request failed with {status}.")
}

fn response_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn number_at(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn parse_key_list(value: &Value) -> Result<Vec<ApiKeyRecord>, String> {
    let values = value
        .as_array()
        .or_else(|| value.get("keys").and_then(Value::as_array))
        .ok_or_else(|| "Accly returned an invalid API key list.".to_string())?;
    values
        .iter()
        .cloned()
        .map(|key| {
            serde_json::from_value(key)
                .map_err(|error| format!("Accly returned an invalid API key: {error}"))
        })
        .collect()
}

fn parse_created_key(value: &Value) -> Result<CreatedApiKey, String> {
    let key: ApiKeyResponse = serde_json::from_value(value.clone())
        .map_err(|error| format!("Accly returned an invalid new API key: {error}"))?;
    if key.full_key.is_empty() {
        return Err("Accly did not return the new API key secret.".to_string());
    }
    Ok(CreatedApiKey {
        record: ApiKeyRecord {
            prefix: key.prefix,
            group_type: key.group_type,
            allowed_tiers: key.allowed_tiers,
            created_at: key.created_at,
        },
        full_key: key.full_key,
    })
}

fn validate_group_type(group_type: &str) -> Result<(), String> {
    if matches!(group_type, "anthropic" | "openai" | "google" | "universal") {
        Ok(())
    } else {
        Err("The requested API-key access group is invalid.".to_string())
    }
}
