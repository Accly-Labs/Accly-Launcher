use keyring::{Entry, Error as KeyringError};
use reqwest::{Client, Method, Response, StatusCode, Url};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const PRODUCTION_KEYCHAIN_SERVICE: &str = "net.accly.launcher";
const LOCAL_KEYCHAIN_SERVICE: &str = "net.accly.launcher.local";
const KEYCHAIN_ACCOUNT: &str = "device-session";
const PRODUCTION_AUTH_ORIGIN: &str = "https://auth.accly.net";
const PRODUCTION_CORE_ORIGIN: &str = "https://core.accly.net";
const MIN_POLL_INTERVAL_SECONDS: u64 = 2;
const SLOW_DOWN_INTERVAL_INCREMENT_SECONDS: u64 = 5;
const SESSION_REVOCATION_TIMEOUT: Duration = Duration::from_secs(3);
const SESSION_REAUTHENTICATION_MESSAGE: &str =
    "Your launcher session has expired. Reconnect to continue.";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSession {
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DeviceAuthorizationPoll {
    Pending {
        #[serde(rename = "retryAfterSeconds")]
        retry_after_seconds: u64,
    },
    Expired,
    Completed {
        session: LauncherSession,
    },
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
struct LauncherSessionResponse {
    session: LauncherSession,
}

#[derive(Deserialize)]
struct DeviceTokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: String,
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
    fn load() -> Result<Self, String> {
        validate_build_profile(cfg!(debug_assertions), is_local_build())?;
        let allow_localhost = is_local_build();
        let auth = validate_endpoint(
            "ACCLY_AUTH_URL",
            &configured_value(
                "ACCLY_AUTH_URL",
                option_env!("ACCLY_AUTH_URL"),
                PRODUCTION_AUTH_ORIGIN,
            ),
            PRODUCTION_AUTH_ORIGIN,
            allow_localhost,
        )?;
        let core = validate_endpoint(
            "ACCLY_CORE_URL",
            &configured_value(
                "ACCLY_CORE_URL",
                option_env!("ACCLY_CORE_URL"),
                PRODUCTION_CORE_ORIGIN,
            ),
            PRODUCTION_CORE_ORIGIN,
            allow_localhost,
        )?;

        if allow_localhost && (!is_localhost_endpoint(&auth) || !is_localhost_endpoint(&core)) {
            return Err(
                "The local launcher profile requires localhost Auth and Core URLs.".to_string(),
            );
        }

        Ok(Self {
            auth,
            core,
            client_id: configured_value(
                "ACCLY_LAUNCHER_CLIENT_ID",
                option_env!("ACCLY_LAUNCHER_CLIENT_ID"),
                "accly-launcher",
            ),
        })
    }

    fn auth_route(&self, route: &str) -> String {
        format!("{}{}", self.auth.trim_end_matches('/'), route)
    }

    fn core_route(&self, route: &str) -> String {
        format!("{}{}", self.core.trim_end_matches('/'), route)
    }

    fn launcher_session_route(&self) -> String {
        self.auth_route("/api/auth/launcher/session")
    }
}

pub fn is_local_build() -> bool {
    cfg!(feature = "local")
}

fn validate_build_profile(is_debug_build: bool, is_local_build: bool) -> Result<(), String> {
    match (is_debug_build, is_local_build) {
        (true, true) | (false, false) => Ok(()),
        (true, false) => Err(
            "Use the local launcher profile for native debug builds so they cannot access production services."
                .to_string(),
        ),
        (false, true) => Err(
            "The local launcher profile may only be built in debug mode.".to_string(),
        ),
    }
}

fn configured_value(name: &str, compiled: Option<&str>, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| compiled.map(ToString::to_string))
        .unwrap_or_else(|| fallback.to_string())
}

fn validate_endpoint(
    name: &str,
    value: &str,
    expected_origin: &str,
    allow_localhost: bool,
) -> Result<String, String> {
    let parsed = Url::parse(value).map_err(|_| format!("{name} must be an absolute HTTPS URL."))?;

    if !is_root_url(&parsed) {
        return Err(format!(
            "{name} must not include a path, query, or fragment."
        ));
    }

    if allow_localhost && is_localhost_url(&parsed) {
        return Ok(parsed.origin().ascii_serialization());
    }

    if parsed.scheme() != "https" || parsed.origin().ascii_serialization() != expected_origin {
        return Err(format!(
            "{name} must use {expected_origin} outside local debug builds."
        ));
    }

    Ok(expected_origin.to_string())
}

fn is_root_url(url: &Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.fragment().is_none()
}

fn is_localhost_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && matches!(
            url.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("::1") | Some("[::1]")
        )
}

fn is_localhost_endpoint(value: &str) -> bool {
    Url::parse(value)
        .map(|url| is_localhost_url(&url))
        .unwrap_or(false)
}

fn validate_verification_uri(
    value: &str,
    auth_origin: &str,
    requires_user_code: bool,
) -> Result<String, String> {
    let parsed = Url::parse(value)
        .map_err(|_| "Auth returned an invalid launcher verification URL.".to_string())?;

    if parsed.origin().ascii_serialization() != auth_origin
        || parsed.path() != "/device"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Auth returned an untrusted launcher verification URL.".to_string());
    }

    let query_pairs = parsed.query_pairs().collect::<Vec<_>>();
    if requires_user_code {
        let valid_user_code = query_pairs.len() == 1
            && query_pairs[0].0 == "user_code"
            && is_valid_user_code(query_pairs[0].1.as_ref());
        if !valid_user_code {
            return Err("Auth returned an invalid launcher verification code URL.".to_string());
        }
    } else if !query_pairs.is_empty() {
        return Err("Auth returned an invalid launcher verification URL.".to_string());
    }

    Ok(parsed.to_string())
}

fn is_valid_user_code(value: &str) -> bool {
    (4..=64).contains(&value.len())
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn default_expires_in() -> u64 {
    900
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
    Entry::new(keychain_service(), KEYCHAIN_ACCOUNT)
        .map_err(|error| format!("Unable to access the system credential store: {error}"))
}

fn keychain_service() -> &'static str {
    if is_local_build() || cfg!(debug_assertions) {
        LOCAL_KEYCHAIN_SERVICE
    } else {
        PRODUCTION_KEYCHAIN_SERVICE
    }
}

fn read_session_token() -> Result<Option<String>, String> {
    let entry = session_entry()?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => Ok(Some(token)),
        Ok(_) => Ok(None),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "Unable to read the launcher session from the system credential store: {error}"
        )),
    }
}

fn store_session_token(token: &str) -> Result<(), String> {
    session_entry()?
        .set_password(token)
        .map_err(|error| format!("Unable to store the launcher session: {error}"))
}

pub async fn get_launcher_session() -> Result<Option<LauncherSession>, String> {
    let Some(token) = read_session_token()? else {
        return Ok(None);
    };

    let endpoints = Endpoints::load()?;
    let response = client()?
        .get(endpoints.launcher_session_route())
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("Unable to verify the launcher session: {error}"))?;

    if response.status() == StatusCode::UNAUTHORIZED {
        delete_session_token()?;
        return Ok(None);
    }

    let session: LauncherSessionResponse = response_json(response).await?;
    Ok(Some(verified_launcher_session(session)?))
}

fn verified_launcher_session(response: LauncherSessionResponse) -> Result<LauncherSession, String> {
    let session = response.session;
    if session.expires_at.as_deref().is_none_or(str::is_empty) {
        return Err("Auth returned a launcher session without an expiry.".to_string());
    }

    Ok(session)
}

pub async fn clear_launcher_session() -> Result<(), String> {
    if let Some(token) = read_session_token()? {
        revoke_launcher_session(&token).await;
    }

    delete_session_token()
}

fn delete_session_token() -> Result<(), String> {
    let entry = session_entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(format!(
            "Unable to remove the launcher session from the system credential store: {error}"
        )),
    }
}

async fn revoke_launcher_session(token: &str) {
    let Ok(endpoints) = Endpoints::load() else {
        return;
    };
    let Ok(http_client) = client() else {
        return;
    };

    let _ = http_client
        .delete(endpoints.launcher_session_route())
        .bearer_auth(token)
        .timeout(SESSION_REVOCATION_TIMEOUT)
        .send()
        .await;
}

pub async fn begin_device_authorization() -> Result<DeviceCode, String> {
    let endpoints = Endpoints::load()?;
    let response = client()?
        .post(endpoints.auth_route("/api/auth/device/code"))
        .json(&json!({
            "client_id": endpoints.client_id,
            "scope": "launcher"
        }))
        .send()
        .await
        .map_err(|error| format!("Unable to start device authorization: {error}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(
            "Accly Launcher sign-in is not enabled on the authentication service yet.".to_string(),
        );
    }
    let payload: DeviceCodeResponse = response_json(response).await?;
    if !is_valid_user_code(&payload.user_code) {
        return Err("Auth returned an invalid launcher verification code.".to_string());
    }

    let verification_uri =
        validate_verification_uri(&payload.verification_uri, &endpoints.auth, false)?;
    let verification_uri_complete = payload
        .verification_uri_complete
        .as_deref()
        .map(|value| validate_verification_uri(value, &endpoints.auth, true))
        .transpose()?;

    Ok(DeviceCode {
        device_code: payload.device_code,
        user_code: payload.user_code,
        verification_uri,
        verification_uri_complete,
        expires_in: payload.expires_in,
        interval: payload.interval.max(MIN_POLL_INTERVAL_SECONDS),
    })
}

pub async fn poll_device_authorization(
    device_code: DeviceCode,
) -> Result<DeviceAuthorizationPoll, String> {
    let endpoints = Endpoints::load()?;
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
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if let Ok(error) = serde_json::from_str::<DeviceTokenErrorResponse>(&body) {
            if let Some(poll) = device_authorization_poll_result(&error, device_code.interval) {
                return Ok(poll);
            }

            return Err(device_token_error_message(&error));
        }

        return Err(response_error_from_body(status, &body));
    }

    let payload: DeviceTokenResponse = response
        .json()
        .await
        .map_err(|error| format!("Accly returned an invalid response: {error}"))?;
    store_session_token(&payload.access_token)?;
    Ok(DeviceAuthorizationPoll::Completed {
        session: LauncherSession { expires_at: None },
    })
}

fn device_authorization_poll_result(
    error: &DeviceTokenErrorResponse,
    interval: u64,
) -> Option<DeviceAuthorizationPoll> {
    match error.error.as_str() {
        "authorization_pending" => Some(DeviceAuthorizationPoll::Pending {
            retry_after_seconds: interval.max(MIN_POLL_INTERVAL_SECONDS),
        }),
        "slow_down" => Some(DeviceAuthorizationPoll::Pending {
            retry_after_seconds: interval
                .max(MIN_POLL_INTERVAL_SECONDS)
                .saturating_add(SLOW_DOWN_INTERVAL_INCREMENT_SECONDS),
        }),
        "expired_token" => Some(DeviceAuthorizationPoll::Expired),
        _ => None,
    }
}

fn device_token_error_message(error: &DeviceTokenErrorResponse) -> String {
    if error.error_description.trim().is_empty() {
        error.error.clone()
    } else {
        error.error_description.clone()
    }
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
        usage: usage_summary(usage),
        keys: parse_key_list(keys)?,
    })
}

fn usage_summary(usage: &Value) -> UsageSummary {
    let request_units = usage.get("requestUnits").unwrap_or(usage);

    UsageSummary {
        daily: number_at(request_units, "daily"),
        used: number_at(request_units, "used"),
        remaining: number_at(request_units, "remaining"),
        percent_used: number_at(request_units, "percentUsed"),
    }
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
    if !update_checks_enabled(is_local_build()) {
        return Ok(UpdateStatus {
            available: false,
            version: None,
            url: None,
        });
    }

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

fn update_checks_enabled(is_local_build: bool) -> bool {
    !is_local_build
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
        read_session_token()?.ok_or_else(|| SESSION_REAUTHENTICATION_MESSAGE.to_string())?;
    let endpoints = Endpoints::load()?;
    let request = client()?
        .request(method, endpoints.core_route(route))
        .bearer_auth(token);
    let request = match body {
        Some(body) => request.json(&body),
        None => request,
    };
    let response = request
        .send()
        .await
        .map_err(|error| format!("Unable to contact Accly: {error}"))?;

    if response.status() == StatusCode::UNAUTHORIZED {
        delete_session_token()?;
        return Err(SESSION_REAUTHENTICATION_MESSAGE.to_string());
    }

    Ok(response)
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
    response_error_from_body(status, &body)
}

fn response_error_from_body(status: StatusCode, body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        device_authorization_poll_result, update_checks_enabled, usage_summary,
        validate_build_profile, validate_endpoint, validate_verification_uri,
        verified_launcher_session, DeviceAuthorizationPoll, DeviceTokenErrorResponse, Endpoints,
        LauncherSessionResponse, PRODUCTION_AUTH_ORIGIN, PRODUCTION_CORE_ORIGIN,
    };

    #[test]
    fn accepts_pinned_production_endpoints_and_local_debug_endpoints() {
        assert_eq!(
            validate_endpoint(
                "ACCLY_AUTH_URL",
                "https://auth.accly.net/",
                PRODUCTION_AUTH_ORIGIN,
                false,
            ),
            Ok(PRODUCTION_AUTH_ORIGIN.to_string())
        );
        assert_eq!(
            validate_endpoint(
                "ACCLY_CORE_URL",
                "http://localhost:4100",
                PRODUCTION_CORE_ORIGIN,
                true,
            ),
            Ok("http://localhost:4100".to_string())
        );
    }

    #[test]
    fn requires_the_local_feature_for_debug_builds() {
        assert!(validate_build_profile(true, true).is_ok());
        assert!(validate_build_profile(false, false).is_ok());
        assert!(validate_build_profile(true, false).is_err());
        assert!(validate_build_profile(false, true).is_err());
    }

    #[test]
    fn skips_github_update_checks_for_the_local_profile() {
        assert!(!update_checks_enabled(true));
        assert!(update_checks_enabled(false));
    }

    #[test]
    fn rejects_unpinned_or_non_root_endpoints() {
        assert!(validate_endpoint(
            "ACCLY_AUTH_URL",
            "http://auth.accly.net",
            PRODUCTION_AUTH_ORIGIN,
            false,
        )
        .is_err());
        assert!(validate_endpoint(
            "ACCLY_CORE_URL",
            "https://core.accly.net/private",
            PRODUCTION_CORE_ORIGIN,
            false,
        )
        .is_err());
        assert!(validate_endpoint(
            "ACCLY_AUTH_URL",
            "https://auth.accly.net.attacker.example",
            PRODUCTION_AUTH_ORIGIN,
            false,
        )
        .is_err());
    }

    #[test]
    fn accepts_only_the_configured_device_verification_urls() {
        assert_eq!(
            validate_verification_uri(
                "https://auth.accly.net/device",
                PRODUCTION_AUTH_ORIGIN,
                false,
            ),
            Ok("https://auth.accly.net/device".to_string())
        );
        assert!(validate_verification_uri(
            "https://auth.accly.net/device?user_code=ACCLYDEV",
            PRODUCTION_AUTH_ORIGIN,
            false,
        )
        .is_err());
        assert_eq!(
            validate_verification_uri(
                "https://auth.accly.net/device?user_code=ACCLY-DEV",
                PRODUCTION_AUTH_ORIGIN,
                true,
            ),
            Ok("https://auth.accly.net/device?user_code=ACCLY-DEV".to_string())
        );
        assert!(validate_verification_uri(
            "https://attacker.example/device?user_code=ACCLY-DEV",
            PRODUCTION_AUTH_ORIGIN,
            true,
        )
        .is_err());
    }

    #[test]
    fn classifies_device_authorization_poll_responses() {
        let pending = DeviceTokenErrorResponse {
            error: "authorization_pending".to_string(),
            error_description: "Authorization pending".to_string(),
        };
        assert_eq!(
            device_authorization_poll_result(&pending, 5),
            Some(DeviceAuthorizationPoll::Pending {
                retry_after_seconds: 5,
            })
        );

        let slow_down = DeviceTokenErrorResponse {
            error: "slow_down".to_string(),
            error_description: "Polling too frequently".to_string(),
        };
        assert_eq!(
            device_authorization_poll_result(&slow_down, 5),
            Some(DeviceAuthorizationPoll::Pending {
                retry_after_seconds: 10,
            })
        );

        let expired = DeviceTokenErrorResponse {
            error: "expired_token".to_string(),
            error_description: "Device code has expired".to_string(),
        };
        assert_eq!(
            device_authorization_poll_result(&expired, 5),
            Some(DeviceAuthorizationPoll::Expired)
        );
    }

    #[test]
    fn serializes_device_poll_results_for_tauri() {
        let payload = serde_json::to_value(DeviceAuthorizationPoll::Pending {
            retry_after_seconds: 10,
        })
        .expect("device authorization poll result should serialize");

        assert_eq!(
            payload,
            json!({ "status": "pending", "retryAfterSeconds": 10 })
        );

        let expired = serde_json::to_value(DeviceAuthorizationPoll::Expired)
            .expect("expired device authorization result should serialize");
        assert_eq!(expired, json!({ "status": "expired" }));
    }

    #[test]
    fn accepts_only_launcher_sessions_with_an_expiry() {
        let valid: LauncherSessionResponse = serde_json::from_value(json!({
            "session": { "expiresAt": "2026-07-21T12:00:00.000Z" }
        }))
        .expect("Auth session response should deserialize");
        assert_eq!(
            verified_launcher_session(valid)
                .expect("expiry should be present")
                .expires_at
                .as_deref(),
            Some("2026-07-21T12:00:00.000Z")
        );

        let missing: LauncherSessionResponse = serde_json::from_value(json!({
            "session": { "expiresAt": null }
        }))
        .expect("missing expiry response should deserialize");
        assert!(verified_launcher_session(missing).is_err());
    }

    #[test]
    fn reads_request_units_from_the_core_usage_response() {
        let usage = json!({
            "date": "2026-07-13",
            "requests": 4,
            "requestUnits": {
                "daily": 4_000,
                "used": 68,
                "remaining": 3_932,
                "percentUsed": 1.7
            }
        });

        let summary = usage_summary(&usage);

        assert_eq!(summary.daily, 4_000.0);
        assert_eq!(summary.used, 68.0);
        assert_eq!(summary.remaining, 3_932.0);
        assert_eq!(summary.percent_used, 1.7);
    }

    #[test]
    fn builds_the_pinned_launcher_session_revocation_route() {
        let endpoints = Endpoints {
            auth: PRODUCTION_AUTH_ORIGIN.to_string(),
            core: PRODUCTION_CORE_ORIGIN.to_string(),
            client_id: "accly-launcher".to_string(),
        };

        assert_eq!(
            endpoints.launcher_session_route(),
            "https://auth.accly.net/api/auth/launcher/session"
        );
    }
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
