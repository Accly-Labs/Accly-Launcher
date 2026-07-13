use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;
use toml_edit::{value, DocumentMut, Item, Table};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfiguration {
    pub agent_id: String,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetection {
    pub id: String,
    pub name: String,
    pub state: String,
    pub installed: bool,
    pub configurable: bool,
    pub config_path: Option<String>,
    pub detail: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureResult {
    pub agent_id: String,
    pub config_path: String,
    pub backup_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub detail: String,
}

#[derive(Clone)]
struct PendingWrite {
    path: PathBuf,
    contents: Vec<u8>,
}

struct Snapshot {
    path: PathBuf,
    original: Option<Vec<u8>>,
    permissions: Option<fs::Permissions>,
}

struct FileTransaction {
    snapshots: Vec<Snapshot>,
    writes: Vec<PendingWrite>,
    backup_dir: PathBuf,
}

impl FileTransaction {
    fn begin(agent_id: &str, writes: Vec<PendingWrite>) -> Result<Self, String> {
        if writes.is_empty() {
            return Err("No configuration files were prepared.".to_string());
        }

        let mut unique_paths = HashSet::new();
        for write in &writes {
            if !unique_paths.insert(write.path.clone()) {
                return Err("An adapter attempted to write the same file twice.".to_string());
            }
            if write.path.exists()
                && fs::symlink_metadata(&write.path)
                    .map_err(|error| {
                        format!("Unable to inspect {}: {error}", write.path.display())
                    })?
                    .file_type()
                    .is_symlink()
            {
                return Err(format!(
                    "Refusing to replace symlinked configuration file {}.",
                    write.path.display()
                ));
            }
        }

        let backup_dir = backup_directory(agent_id)?;
        fs::create_dir_all(&backup_dir)
            .map_err(|error| format!("Unable to create backup directory: {error}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("Unable to protect backup directory: {error}"))?;
        }

        let mut snapshots = Vec::with_capacity(writes.len());
        for (index, write) in writes.iter().enumerate() {
            let original =
                if write.path.exists() {
                    Some(fs::read(&write.path).map_err(|error| {
                        format!("Unable to read {}: {error}", write.path.display())
                    })?)
                } else {
                    None
                };
            let permissions = if write.path.exists() {
                Some(
                    fs::metadata(&write.path)
                        .map_err(|error| {
                            format!("Unable to inspect {}: {error}", write.path.display())
                        })?
                        .permissions(),
                )
            } else {
                None
            };
            let filename = write
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("config");
            let backup_file = backup_dir.join(format!("{index:02}-{filename}.bak"));

            match &original {
                Some(bytes) => fs::write(&backup_file, bytes).map_err(|error| {
                    format!("Unable to back up {}: {error}", write.path.display())
                })?,
                None => fs::write(
                    backup_dir.join(format!("{index:02}-{filename}.absent")),
                    b"absent",
                )
                .map_err(|error| format!("Unable to record new configuration file: {error}"))?,
            }

            snapshots.push(Snapshot {
                path: write.path.clone(),
                original,
                permissions,
            });
        }

        Ok(Self {
            snapshots,
            writes,
            backup_dir,
        })
    }

    fn apply(&self) -> Result<(), String> {
        for (write, snapshot) in self.writes.iter().zip(&self.snapshots) {
            atomic_write(&write.path, &write.contents, snapshot.permissions.as_ref())?;
        }
        Ok(())
    }

    fn restore(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for snapshot in self.snapshots.iter().rev() {
            let result = match &snapshot.original {
                Some(bytes) => atomic_write(&snapshot.path, bytes, snapshot.permissions.as_ref()),
                None if snapshot.path.exists() => {
                    fs::remove_file(&snapshot.path).map_err(|error| {
                        format!("Unable to remove {}: {error}", snapshot.path.display())
                    })
                }
                None => Ok(()),
            };
            if let Err(error) = result {
                errors.push(error);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(" "))
        }
    }
}

trait AgentAdapter {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn icon(&self) -> &'static str;
    fn config_paths(&self, home: &Path) -> Vec<PathBuf>;
    fn command(&self) -> Option<&'static str>;
    fn app_paths(&self, _home: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    fn configurable(&self) -> bool {
        true
    }
    fn prepare(
        &self,
        configuration: &AgentConfiguration,
        home: &Path,
    ) -> Result<Vec<PendingWrite>, String>;
    fn validate(&self, home: &Path) -> Result<(), String>;

    fn primary_path(&self, home: &Path) -> PathBuf {
        let paths = self.config_paths(home);
        paths
            .iter()
            .find(|path| path.exists())
            .cloned()
            .or_else(|| paths.first().cloned())
            .expect("agent adapters must declare at least one configuration path")
    }

    fn installed(&self, home: &Path) -> (bool, bool) {
        let command_status = self.command().map(command_status);
        let app_found = self.app_paths(home).iter().any(|path| path.exists());
        let config_found = self.config_paths(home).iter().any(|path| path.exists());

        match command_status {
            Some(CommandStatus::Ready) => (true, false),
            Some(CommandStatus::Broken) => (true, true),
            Some(CommandStatus::Missing) => (app_found || config_found, false),
            None => (app_found || config_found, false),
        }
    }

    fn detection(&self, home: &Path) -> AgentDetection {
        let primary_path = self.primary_path(home);
        let (installed, broken) = self.installed(home);
        let configurable = self.configurable();
        let (state, detail) = if !configurable {
            (
                "unsupported".to_string(),
                "No safe gateway config detected".to_string(),
            )
        } else if broken {
            (
                "broken".to_string(),
                "Installed, but unavailable".to_string(),
            )
        } else if !installed {
            ("missing".to_string(), "Not installed".to_string())
        } else if self.validate(home).is_ok() {
            ("ready".to_string(), "Ready to connect".to_string())
        } else {
            ("installed".to_string(), "Detected".to_string())
        };

        AgentDetection {
            id: self.id().to_string(),
            name: self.name().to_string(),
            state,
            installed,
            configurable,
            config_path: Some(display_path(&primary_path, home)),
            detail,
            icon: self.icon().to_string(),
        }
    }
}

enum CommandStatus {
    Ready,
    Broken,
    Missing,
}

fn command_status(command: &str) -> CommandStatus {
    match Command::new(command).arg("--version").output() {
        Ok(output) if output.status.success() => CommandStatus::Ready,
        Ok(_) => CommandStatus::Broken,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => CommandStatus::Missing,
        Err(_) => CommandStatus::Broken,
    }
}

struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn name(&self) -> &'static str {
        "Codex"
    }

    fn icon(&self) -> &'static str {
        "/codex-color.svg"
    }

    fn config_paths(&self, home: &Path) -> Vec<PathBuf> {
        let directory = home.join(".codex");
        vec![directory.join("config.toml"), directory.join("auth.json")]
    }

    fn command(&self) -> Option<&'static str> {
        Some("codex")
    }

    fn prepare(
        &self,
        configuration: &AgentConfiguration,
        home: &Path,
    ) -> Result<Vec<PendingWrite>, String> {
        let paths = self.config_paths(home);
        let config_path = &paths[0];
        let auth_path = &paths[1];
        let mut document = read_toml(config_path)?;
        let endpoint = api_base_url(&configuration.endpoint)?;

        document["model"] = value(configuration.model.clone());
        document["model_provider"] = value("accly");
        let providers = ensure_toml_table(&mut document, "model_providers")?;
        let provider = ensure_toml_subtable(providers, "accly")?;
        provider["base_url"] = value(endpoint);
        provider["env_key"] = value("OPENAI_API_KEY");
        provider["wire_api"] = value("responses");
        provider["requires_openai_auth"] = value(true);

        let mut auth = read_json(auth_path)?;
        let auth_object = json_object_mut(&mut auth, "Codex authentication")?;
        auth_object.insert(
            "OPENAI_API_KEY".to_string(),
            Value::String(configuration.api_key.clone()),
        );

        Ok(vec![
            PendingWrite {
                path: config_path.clone(),
                contents: document.to_string().into_bytes(),
            },
            PendingWrite {
                path: auth_path.clone(),
                contents: serialize_json(&auth, auth_path)?,
            },
        ])
    }

    fn validate(&self, home: &Path) -> Result<(), String> {
        let paths = self.config_paths(home);
        let config = read_toml(&paths[0])?;
        let provider = config
            .get("model_provider")
            .and_then(Item::as_str)
            .ok_or_else(|| "Codex model provider is missing.".to_string())?;
        if provider != "accly" {
            return Err("Codex is not configured for Accly.".to_string());
        }
        let base_url = config
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get("accly"))
            .and_then(Item::as_table)
            .and_then(|provider| provider.get("base_url"))
            .and_then(Item::as_str)
            .ok_or_else(|| "Codex Accly endpoint is missing.".to_string())?;
        if base_url.is_empty() {
            return Err("Codex Accly endpoint is empty.".to_string());
        }
        let auth = read_json(&paths[1])?;
        if auth
            .get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err("Codex API key is missing.".to_string());
        }
        Ok(())
    }
}

struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn icon(&self) -> &'static str {
        "/claudecode-color.svg"
    }

    fn config_paths(&self, home: &Path) -> Vec<PathBuf> {
        vec![home.join(".claude").join("settings.json")]
    }

    fn command(&self) -> Option<&'static str> {
        Some("claude")
    }

    fn prepare(
        &self,
        configuration: &AgentConfiguration,
        home: &Path,
    ) -> Result<Vec<PendingWrite>, String> {
        let path = self.primary_path(home);
        let mut settings = read_json(&path)?;
        let settings_object = json_object_mut(&mut settings, "Claude settings")?;
        let env = ensure_json_object(settings_object, "env")?;
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            Value::String(api_base_url(&configuration.endpoint)?),
        );
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            Value::String(configuration.api_key.clone()),
        );
        env.insert(
            "ANTHROPIC_MODEL".to_string(),
            Value::String(configuration.model.clone()),
        );

        Ok(vec![PendingWrite {
            path: path.clone(),
            contents: serialize_json(&settings, &path)?,
        }])
    }

    fn validate(&self, home: &Path) -> Result<(), String> {
        let path = self.primary_path(home);
        let settings = read_json(&path)?;
        let env = settings
            .get("env")
            .and_then(Value::as_object)
            .ok_or_else(|| "Claude settings environment is missing.".to_string())?;
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_MODEL",
        ] {
            if env
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(format!("Claude setting {key} is missing."));
            }
        }
        Ok(())
    }
}

struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn id(&self) -> &'static str {
        "gemini-cli"
    }

    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn icon(&self) -> &'static str {
        ""
    }

    fn config_paths(&self, home: &Path) -> Vec<PathBuf> {
        let directory = home.join(".gemini");
        vec![directory.join(".env"), directory.join("settings.json")]
    }

    fn command(&self) -> Option<&'static str> {
        Some("gemini")
    }

    fn prepare(
        &self,
        configuration: &AgentConfiguration,
        home: &Path,
    ) -> Result<Vec<PendingWrite>, String> {
        let paths = self.config_paths(home);
        let env_path = &paths[0];
        let settings_path = &paths[1];
        let existing_env = read_text(env_path)?;
        let endpoint = api_base_url(&configuration.endpoint)?;
        let env = patch_env(
            &existing_env,
            &[
                ("GOOGLE_GEMINI_BASE_URL", endpoint),
                ("GEMINI_API_KEY", configuration.api_key.clone()),
                ("GEMINI_MODEL", configuration.model.clone()),
            ],
        )?;

        let mut settings = read_json(settings_path)?;
        let root = json_object_mut(&mut settings, "Gemini settings")?;
        let security = ensure_json_object(root, "security")?;
        let auth = ensure_json_object(security, "auth")?;
        auth.insert(
            "selectedType".to_string(),
            Value::String("gemini-api-key".to_string()),
        );

        Ok(vec![
            PendingWrite {
                path: env_path.clone(),
                contents: env.into_bytes(),
            },
            PendingWrite {
                path: settings_path.clone(),
                contents: serialize_json(&settings, settings_path)?,
            },
        ])
    }

    fn validate(&self, home: &Path) -> Result<(), String> {
        let paths = self.config_paths(home);
        let values = parse_env(&read_text(&paths[0])?);
        for key in ["GOOGLE_GEMINI_BASE_URL", "GEMINI_API_KEY", "GEMINI_MODEL"] {
            if values.get(key).filter(|value| !value.is_empty()).is_none() {
                return Err(format!("Gemini setting {key} is missing."));
            }
        }
        let settings = read_json(&paths[1])?;
        if settings
            .pointer("/security/auth/selectedType")
            .and_then(Value::as_str)
            != Some("gemini-api-key")
        {
            return Err("Gemini API-key authentication is not selected.".to_string());
        }
        Ok(())
    }
}

struct OpenCodeAdapter;

impl AgentAdapter for OpenCodeAdapter {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn name(&self) -> &'static str {
        "OpenCode"
    }

    fn icon(&self) -> &'static str {
        "/opencode.svg"
    }

    fn config_paths(&self, home: &Path) -> Vec<PathBuf> {
        vec![home.join(".config").join("opencode").join("opencode.json")]
    }

    fn command(&self) -> Option<&'static str> {
        Some("opencode")
    }

    fn prepare(
        &self,
        configuration: &AgentConfiguration,
        home: &Path,
    ) -> Result<Vec<PendingWrite>, String> {
        let path = self.primary_path(home);
        let mut config = read_json(&path)?;
        let root = json_object_mut(&mut config, "OpenCode config")?;
        let providers = ensure_json_object(root, "provider")?;
        let accly = ensure_json_object(providers, "accly")?;
        accly.insert(
            "npm".to_string(),
            Value::String("@ai-sdk/openai-compatible".to_string()),
        );
        let options = ensure_json_object(accly, "options")?;
        options.insert(
            "baseURL".to_string(),
            Value::String(api_base_url(&configuration.endpoint)?),
        );
        options.insert(
            "apiKey".to_string(),
            Value::String(configuration.api_key.clone()),
        );
        let models = ensure_json_object(accly, "models")?;
        let model = ensure_json_object(models, &configuration.model)?;
        model
            .entry("name".to_string())
            .or_insert_with(|| Value::String(configuration.model.clone()));

        Ok(vec![PendingWrite {
            path: path.clone(),
            contents: serialize_json(&config, &path)?,
        }])
    }

    fn validate(&self, home: &Path) -> Result<(), String> {
        let path = self.primary_path(home);
        let config = read_json(&path)?;
        let options = config
            .pointer("/provider/accly/options")
            .and_then(Value::as_object)
            .ok_or_else(|| "OpenCode Accly provider is missing.".to_string())?;
        for key in ["baseURL", "apiKey"] {
            if options
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(format!("OpenCode setting {key} is missing."));
            }
        }
        Ok(())
    }
}

struct CursorAdapter;

impl AgentAdapter for CursorAdapter {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn name(&self) -> &'static str {
        "Cursor"
    }

    fn icon(&self) -> &'static str {
        "/cursor.svg"
    }

    fn config_paths(&self, home: &Path) -> Vec<PathBuf> {
        vec![home
            .join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("User")
            .join("settings.json")]
    }

    fn command(&self) -> Option<&'static str> {
        None
    }

    fn app_paths(&self, home: &Path) -> Vec<PathBuf> {
        vec![
            home.join("Applications").join("Cursor.app"),
            PathBuf::from("/Applications/Cursor.app"),
        ]
    }

    fn configurable(&self) -> bool {
        false
    }

    fn prepare(&self, _: &AgentConfiguration, _: &Path) -> Result<Vec<PendingWrite>, String> {
        Err("Cursor does not expose a verified custom-gateway configuration contract.".to_string())
    }

    fn validate(&self, _: &Path) -> Result<(), String> {
        Err("Cursor custom gateway configuration is unavailable.".to_string())
    }
}

struct WindsurfAdapter;

impl AgentAdapter for WindsurfAdapter {
    fn id(&self) -> &'static str {
        "windsurf"
    }

    fn name(&self) -> &'static str {
        "Windsurf"
    }

    fn icon(&self) -> &'static str {
        "/windsurf.svg"
    }

    fn config_paths(&self, home: &Path) -> Vec<PathBuf> {
        vec![home
            .join("Library")
            .join("Application Support")
            .join("Windsurf")
            .join("User")
            .join("settings.json")]
    }

    fn command(&self) -> Option<&'static str> {
        None
    }

    fn app_paths(&self, home: &Path) -> Vec<PathBuf> {
        vec![
            home.join("Applications").join("Windsurf.app"),
            PathBuf::from("/Applications/Windsurf.app"),
        ]
    }

    fn configurable(&self) -> bool {
        false
    }

    fn prepare(&self, _: &AgentConfiguration, _: &Path) -> Result<Vec<PendingWrite>, String> {
        Err(
            "Windsurf does not expose a verified custom-gateway configuration contract."
                .to_string(),
        )
    }

    fn validate(&self, _: &Path) -> Result<(), String> {
        Err("Windsurf custom gateway configuration is unavailable.".to_string())
    }
}

fn adapters() -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(CodexAdapter),
        Box::new(ClaudeCodeAdapter),
        Box::new(GeminiAdapter),
        Box::new(OpenCodeAdapter),
        Box::new(CursorAdapter),
        Box::new(WindsurfAdapter),
    ]
}

fn adapter_by_id(id: &str) -> Option<Box<dyn AgentAdapter>> {
    adapters().into_iter().find(|adapter| adapter.id() == id)
}

pub fn detect_agents() -> Result<Vec<AgentDetection>, String> {
    let home = home_directory()?;
    Ok(adapters()
        .iter()
        .map(|adapter| adapter.detection(&home))
        .collect())
}

pub fn configure_agent(configuration: AgentConfiguration) -> Result<ConfigureResult, String> {
    validate_configuration(&configuration)?;
    let home = home_directory()?;
    let adapter = adapter_by_id(&configuration.agent_id)
        .ok_or_else(|| "This agent is not supported by Accly Launcher.".to_string())?;
    if !adapter.configurable() {
        return Err(format!(
            "{} does not expose a verified custom-gateway configuration contract.",
            adapter.name()
        ));
    }

    let writes = adapter.prepare(&configuration, &home)?;
    let transaction = FileTransaction::begin(adapter.id(), writes)?;
    if let Err(error) = transaction.apply() {
        let _ = transaction.restore();
        return Err(error);
    }
    if let Err(error) = adapter.validate(&home) {
        let restore_error = transaction.restore().err();
        return Err(match restore_error {
            Some(restore_error) => {
                format!("{error} Configuration rollback also failed: {restore_error}")
            }
            None => format!("{error} The previous configuration was restored."),
        });
    }

    Ok(ConfigureResult {
        agent_id: adapter.id().to_string(),
        config_path: display_path(&adapter.primary_path(&home), &home),
        backup_path: transaction.backup_dir.display().to_string(),
        message: format!("{} is connected to Accly.", adapter.name()),
    })
}

pub fn validate_agent(agent_id: String) -> Result<ValidationResult, String> {
    let home = home_directory()?;
    let adapter = adapter_by_id(&agent_id)
        .ok_or_else(|| "This agent is not supported by Accly Launcher.".to_string())?;
    match adapter.validate(&home) {
        Ok(()) => Ok(ValidationResult {
            valid: true,
            detail: "Configuration is valid.".to_string(),
        }),
        Err(detail) => Ok(ValidationResult {
            valid: false,
            detail,
        }),
    }
}

fn home_directory() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "Unable to determine the home directory.".to_string())
}

fn backup_directory(agent_id: &str) -> Result<PathBuf, String> {
    let base = dirs::data_dir().unwrap_or(home_directory()?);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Unable to create backup timestamp: {error}"))?
        .as_millis();
    Ok(base
        .join("Accly Launcher")
        .join("backups")
        .join(agent_id)
        .join(timestamp.to_string()))
}

fn display_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

fn atomic_write(
    path: &Path,
    contents: &[u8],
    existing_permissions: Option<&fs::Permissions>,
) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Configuration path {} has no parent directory.",
            path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Unable to create {}: {error}", parent.display()))?;

    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| format!("Unable to create temporary config file: {error}"))?;
    temporary
        .write_all(contents)
        .map_err(|error| format!("Unable to write temporary config file: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("Unable to flush temporary config file: {error}"))?;

    if let Some(permissions) = existing_permissions {
        temporary
            .as_file()
            .set_permissions(permissions.clone())
            .map_err(|error| format!("Unable to preserve configuration permissions: {error}"))?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("Unable to protect configuration file: {error}"))?;
        }
    }

    temporary
        .persist(path)
        .map_err(|error| format!("Unable to replace {}: {}", path.display(), error.error))?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn validate_configuration(configuration: &AgentConfiguration) -> Result<(), String> {
    api_base_url(&configuration.endpoint)?;
    if configuration.api_key.trim().is_empty() || configuration.api_key.contains('\n') {
        return Err("The API key is invalid.".to_string());
    }
    if configuration.model.trim().is_empty() || configuration.model.contains('\n') {
        return Err("The selected model is invalid.".to_string());
    }
    Ok(())
}

fn api_base_url(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
        return Err("The Accly endpoint must start with http:// or https://.".to_string());
    }
    if endpoint.contains(char::is_whitespace) {
        return Err("The Accly endpoint contains whitespace.".to_string());
    }
    if endpoint.ends_with("/v1") {
        Ok(endpoint.to_string())
    } else {
        Ok(format!("{endpoint}/v1"))
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let bytes =
        fs::read(path).map_err(|error| format!("Unable to read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "{} is not strict JSON and was left unchanged: {error}",
            path.display()
        )
    })
}

fn serialize_json(value: &Value, path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Unable to serialize {}: {error}", path.display()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_toml(path: &Path) -> Result<DocumentMut, String> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Unable to read {}: {error}", path.display()))?;
    contents
        .parse::<DocumentMut>()
        .map_err(|error| format!("{} is not valid TOML: {error}", path.display()))
}

fn read_text(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|error| format!("Unable to read {}: {error}", path.display()))
}

fn json_object_mut<'a>(
    value: &'a mut Value,
    label: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("{label} must be a JSON object."))
}

fn ensure_json_object<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    let value = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value
        .as_object_mut()
        .ok_or_else(|| format!("{key} must be a JSON object to preserve existing settings."))
}

fn ensure_toml_table<'a>(
    document: &'a mut DocumentMut,
    key: &str,
) -> Result<&'a mut Table, String> {
    if document.get(key).is_none() {
        document[key] = Item::Table(Table::new());
    }
    document
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("{key} must be a TOML table to preserve existing settings."))
}

fn ensure_toml_subtable<'a>(table: &'a mut Table, key: &str) -> Result<&'a mut Table, String> {
    if table.get(key).is_none() {
        table.insert(key, Item::Table(Table::new()));
    }
    table
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("{key} must be a TOML table to preserve existing settings."))
}

fn patch_env(existing: &str, values: &[(&str, String)]) -> Result<String, String> {
    let mut lines: Vec<String> = existing.lines().map(ToString::to_string).collect();
    for (key, value) in values {
        if value.contains('\n') || value.contains('\r') {
            return Err(format!("{key} contains an invalid newline."));
        }
        let replacement = format!("{key}={value}");
        let matching_line = lines.iter().position(|line| {
            let trimmed = line.trim_start();
            trimmed
                .strip_prefix(*key)
                .is_some_and(|remainder| remainder.starts_with('='))
        });
        match matching_line {
            Some(index) => lines[index] = replacement,
            None => lines.push(replacement),
        }
    }
    Ok(format!("{}\n", lines.join("\n")))
}

fn parse_env(contents: &str) -> std::collections::HashMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=').map(|(key, value)| {
                (
                    key.trim().to_string(),
                    value.trim().trim_matches('"').to_string(),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{api_base_url, parse_env, patch_env};

    #[test]
    fn appends_v1_to_gateway_base_url() {
        assert_eq!(
            api_base_url("https://api.accly.net").unwrap(),
            "https://api.accly.net/v1"
        );
    }

    #[test]
    fn patches_only_the_requested_env_variables() {
        let patched = patch_env(
            "# keep\nOTHER=value\nGEMINI_MODEL=old\n",
            &[("GEMINI_MODEL", "new".into())],
        )
        .unwrap();
        assert!(patched.contains("# keep"));
        assert!(patched.contains("OTHER=value"));
        assert_eq!(parse_env(&patched).get("GEMINI_MODEL").unwrap(), "new");
    }
}
