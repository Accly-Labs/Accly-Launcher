use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PACKAGE_MANAGER_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CAPTURED_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostPlatform {
    Macos,
    Windows,
    Linux,
    Other,
}

impl HostPlatform {
    pub(crate) fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallationSource {
    Npm,
    Nvm,
    Fnm,
    Mise,
    Pnpm,
    Homebrew,
    Volta,
    Bun,
    Scoop,
    Native,
    System,
}

impl InstallationSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Nvm => "nvm",
            Self::Fnm => "fnm",
            Self::Mise => "mise",
            Self::Pnpm => "pnpm",
            Self::Homebrew => "Homebrew",
            Self::Volta => "Volta",
            Self::Bun => "Bun",
            Self::Scoop => "Scoop",
            Self::Native => "native installer",
            Self::System => "system",
        }
    }

    fn package_manager(self) -> Option<&'static str> {
        match self {
            Self::Npm | Self::Nvm | Self::Fnm | Self::Mise => Some("npm"),
            Self::Pnpm => Some("pnpm"),
            Self::Homebrew
            | Self::Volta
            | Self::Bun
            | Self::Scoop
            | Self::Native
            | Self::System => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommandInstallation {
    pub(crate) path: PathBuf,
    pub(crate) version: Option<String>,
    pub(crate) runnable: bool,
    pub(crate) detail: Option<String>,
    pub(crate) source: InstallationSource,
}

impl CommandInstallation {
    pub(crate) fn can_update(&self) -> bool {
        self.runnable && self.source.package_manager().is_some()
    }

    pub(crate) fn can_repair(&self) -> bool {
        !self.runnable && self.source.package_manager().is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CommandReport {
    pub(crate) installs: Vec<CommandInstallation>,
}

impl CommandReport {
    pub(crate) fn primary(&self) -> Option<&CommandInstallation> {
        self.installs.first()
    }

    pub(crate) fn has_conflict(&self) -> bool {
        self.installs.len() > 1
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PackageManagerRun {
    pub(crate) executable: PathBuf,
    manager: &'static str,
    search_path: OsString,
}

pub(crate) fn inspect_command(
    command: &str,
    home: &Path,
    extra_search_paths: &[PathBuf],
) -> CommandReport {
    let platform = HostPlatform::current();
    let search_paths = binary_search_paths(home, platform, extra_search_paths);
    let search_path = env::join_paths(&search_paths).unwrap_or_default();
    let mut seen = HashSet::new();
    let mut installs = Vec::new();

    for directory in &search_paths {
        for candidate in executable_candidates(command, directory, platform) {
            if !candidate.is_file() {
                continue;
            }

            let real_path = std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
            if !seen.insert(real_path.clone()) {
                continue;
            }

            let (version, runnable, detail) = inspect_executable(&candidate, &search_path);
            installs.push(CommandInstallation {
                source: infer_install_source(&candidate, &real_path),
                path: candidate,
                version,
                runnable,
                detail,
            });
        }
    }

    CommandReport { installs }
}

pub(crate) fn install_package(package: &str, home: &Path) -> Result<PackageManagerRun, String> {
    let npm = resolve_package_manager("npm", home, None)
        .ok_or_else(|| missing_runtime_error("Node.js and npm"))?;
    let run = package_manager_run(npm, "npm", home);
    run_package_manager(&run, package)?;
    Ok(run)
}

pub(crate) fn update_package(
    package: &str,
    installation: &CommandInstallation,
    home: &Path,
) -> Result<PackageManagerRun, String> {
    let manager = installation.source.package_manager().ok_or_else(|| {
        format!(
            "{} is managed by {}. Update it with its original installer, then scan again.",
            installation.path.display(),
            installation.source.label()
        )
    })?;
    let run = resolve_update_package_manager(manager, home, installation).ok_or_else(|| {
        format!(
            "{} is managed by {}. Accly Launcher could not verify its matching {manager} global directory, so it will not update a different installation. Update it with its original package manager, then scan again.",
            installation.path.display(),
            installation.source.label(),
        )
    })?;
    run_package_manager(&run, package)?;
    Ok(run)
}

pub(crate) fn package_manager_search_paths(run: &PackageManagerRun) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(parent) = run.executable.parent() {
        push_unique_path(&mut paths, parent.to_path_buf());
    }
    if let Some(global_bin) = package_manager_global_bin(run) {
        push_unique_path(&mut paths, global_bin);
    }
    paths
}

fn package_manager_run(
    executable: PathBuf,
    manager: &'static str,
    home: &Path,
) -> PackageManagerRun {
    let extra_search_paths = executable
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .collect::<Vec<_>>();
    let search_paths = binary_search_paths(home, HostPlatform::current(), &extra_search_paths);

    PackageManagerRun {
        executable,
        manager,
        search_path: env::join_paths(search_paths).unwrap_or_default(),
    }
}

fn run_package_manager(run: &PackageManagerRun, package: &str) -> Result<(), String> {
    let package_spec = format!("{package}@latest");
    let mut command = Command::new(&run.executable);
    match run.manager {
        "npm" => command.args(["install", "--global", package_spec.as_str()]),
        "pnpm" => command.args(["add", "--global", package_spec.as_str()]),
        _ => return Err(format!("Unsupported package manager: {}", run.manager)),
    };
    command.env("PATH", &run.search_path);

    let output = match run_command(command, PACKAGE_MANAGER_TIMEOUT) {
        Ok(CommandRun::Completed(output)) => output,
        Ok(CommandRun::TimedOut { stdout, stderr }) => {
            let detail = output_detail_from_bytes(&stdout, &stderr);
            return Err(timeout_message(
                run.manager,
                PACKAGE_MANAGER_TIMEOUT,
                &detail,
            ));
        }
        Err(error) => return Err(format!("Unable to run {}: {error}", run.manager)),
    };

    if output.status.success() {
        return Ok(());
    }

    let detail = output_detail(&output);
    if detail.is_empty() {
        Err(format!(
            "{} failed with exit code {:?}.",
            run.manager,
            output.status.code()
        ))
    } else {
        Err(detail)
    }
}

enum CommandRun {
    Completed(Output),
    TimedOut { stdout: Vec<u8>, stderr: Vec<u8> },
}

fn run_command(mut command: Command, timeout: Duration) -> Result<CommandRun, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = thread::spawn(move || read_limited(stdout));
    let stderr_reader = thread::spawn(move || read_limited(stderr));
    let started = Instant::now();

    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let status = child
                    .wait()
                    .map_err(|error| format!("Unable to stop timed out command: {error}"))?;
                break (status, true);
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("Unable to monitor command: {error}"));
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if timed_out {
        Ok(CommandRun::TimedOut { stdout, stderr })
    } else {
        Ok(CommandRun::Completed(Output {
            status,
            stdout,
            stderr,
        }))
    }
}

fn read_limited<R>(mut reader: R) -> Vec<u8>
where
    R: Read + Send + 'static,
{
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 4 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let remaining = MAX_CAPTURED_OUTPUT_BYTES.saturating_sub(captured.len());
                captured.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    }
    captured
}

fn timeout_message(manager: &str, timeout: Duration, detail: &str) -> String {
    let message = format!("{manager} timed out after {} seconds.", timeout.as_secs());
    if detail.is_empty() {
        message
    } else {
        format!("{message} {detail}")
    }
}

fn resolve_package_manager(
    manager: &str,
    home: &Path,
    preferred_directory: Option<&Path>,
) -> Option<PathBuf> {
    let platform = HostPlatform::current();
    if let Some(directory) = preferred_directory {
        let candidate = executable_candidates(manager, directory, platform)
            .into_iter()
            .find(|path| path.is_file());
        if candidate.is_some() {
            return candidate;
        }
    }

    inspect_command(manager, home, &[])
        .primary()
        .map(|installation| installation.path.clone())
}

fn resolve_update_package_manager(
    manager: &'static str,
    home: &Path,
    installation: &CommandInstallation,
) -> Option<PackageManagerRun> {
    let target_directory = installation.path.parent()?;
    let mut candidates = Vec::new();
    let platform = HostPlatform::current();
    if let Some(sibling) = executable_candidates(manager, target_directory, platform)
        .into_iter()
        .find(|path| path.is_file())
    {
        push_unique_path(&mut candidates, sibling);
    }
    for manager_installation in inspect_command(manager, home, &[]).installs {
        push_unique_path(&mut candidates, manager_installation.path);
    }

    candidates.into_iter().find_map(|executable| {
        let run = package_manager_run(executable, manager, home);
        let global_bin = package_manager_global_bin(&run)?;
        paths_match(&global_bin, target_directory).then_some(run)
    })
}

fn paths_match(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn missing_runtime_error(runtime: &str) -> String {
    format!("{runtime} was not found. Install the supported runtime first, then scan again.")
}

#[cfg(not(target_os = "windows"))]
fn login_shell_path() -> Option<OsString> {
    let shell = env::var("SHELL")
        .ok()
        .filter(|shell| is_valid_shell(shell))
        .unwrap_or_else(|| "/bin/sh".to_string());
    let mut command = Command::new(&shell);
    command
        .arg(default_flag_for_shell(&shell))
        .arg("/usr/bin/env")
        .stdin(Stdio::null());
    let output = match run_command(command, Duration::from_secs(2)).ok()? {
        CommandRun::Completed(output) if output.status.success() => output,
        _ => return None,
    };
    let stdout = decode_output(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("PATH="))
        .find(|path| path.starts_with('/'))
        .map(OsString::from)
}

#[cfg(not(target_os = "windows"))]
fn is_valid_shell(shell: &str) -> bool {
    matches!(
        shell.rsplit('/').next().unwrap_or(shell),
        "sh" | "bash" | "zsh" | "fish" | "dash"
    )
}

#[cfg(not(target_os = "windows"))]
fn default_flag_for_shell(shell: &str) -> &'static str {
    match shell.rsplit('/').next().unwrap_or(shell) {
        "dash" | "sh" => "-c",
        "fish" => "-lc",
        _ => "-lic",
    }
}

fn binary_search_paths(
    home: &Path,
    platform: HostPlatform,
    extra_search_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let mut path_entries = Vec::new();
    #[cfg(not(target_os = "windows"))]
    if let Some(login_path) = login_shell_path() {
        for path in env::split_paths(&login_path) {
            push_unique_path(&mut path_entries, path);
        }
    }
    if let Some(inherited_path) = env::var_os("PATH") {
        for path in env::split_paths(&inherited_path) {
            push_unique_path(&mut path_entries, path);
        }
    }
    let app_data = env::var_os("APPDATA").map(PathBuf::from);
    let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program_files = env::var_os("ProgramFiles").map(PathBuf::from);
    let mut paths = binary_search_paths_for(
        home,
        platform,
        &path_entries,
        extra_search_paths,
        app_data.as_deref(),
        local_app_data.as_deref(),
        program_files.as_deref(),
    );

    for path in runtime_manager_paths(home, platform) {
        push_unique_path(&mut paths, path);
    }
    extend_environment_search_paths(&mut paths, platform);

    paths
}

fn binary_search_paths_for(
    home: &Path,
    platform: HostPlatform,
    path_entries: &[PathBuf],
    extra_search_paths: &[PathBuf],
    app_data: Option<&Path>,
    local_app_data: Option<&Path>,
    program_files: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for path in path_entries {
        push_unique_path(&mut paths, path.clone());
    }
    for path in extra_search_paths {
        push_unique_path(&mut paths, path.clone());
    }

    match platform {
        HostPlatform::Macos => {
            for path in [
                home.join(".local/bin"),
                home.join(".npm-global/bin"),
                home.join(".volta/bin"),
                home.join(".bun/bin"),
                home.join(".asdf/shims"),
                home.join(".local/share/mise/shims"),
                home.join(".mise/shims"),
                home.join("Library/pnpm"),
                home.join("bin"),
                home.join(".opencode/bin"),
                home.join("go/bin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ] {
                push_unique_path(&mut paths, path);
            }
        }
        HostPlatform::Linux => {
            for path in [
                home.join(".local/bin"),
                home.join(".npm-global/bin"),
                home.join(".volta/bin"),
                home.join(".bun/bin"),
                home.join(".asdf/shims"),
                home.join(".local/share/mise/shims"),
                home.join(".mise/shims"),
                home.join(".local/share/pnpm"),
                home.join("bin"),
                home.join(".opencode/bin"),
                home.join("go/bin"),
                PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ] {
                push_unique_path(&mut paths, path);
            }
        }
        HostPlatform::Windows => {
            let app_data = app_data
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join("AppData/Roaming"));
            let local_app_data = local_app_data
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join("AppData/Local"));
            let program_files = program_files
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
            for path in [
                app_data.join("npm"),
                local_app_data.join("pnpm"),
                local_app_data.join("Volta/bin"),
                local_app_data.join("Programs/OpenAI/Codex/bin"),
                local_app_data.join("Programs/claude"),
                home.join("scoop/shims"),
                program_files.join("nodejs"),
            ] {
                push_unique_path(&mut paths, path);
            }
        }
        HostPlatform::Other => {}
    }

    paths
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidate.as_os_str().is_empty() && !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

fn runtime_manager_paths(home: &Path, platform: HostPlatform) -> Vec<PathBuf> {
    if !matches!(platform, HostPlatform::Macos | HostPlatform::Linux) {
        return Vec::new();
    }

    let mut paths = Vec::new();
    let mut roots = vec![
        (home.join(".nvm/versions/node"), PathBuf::from("bin")),
        (
            home.join(".fnm/node-versions"),
            PathBuf::from("installation/bin"),
        ),
        (
            home.join(".local/share/mise/installs/node"),
            PathBuf::from("bin"),
        ),
        (home.join(".mise/installs/node"), PathBuf::from("bin")),
        (
            home.join(".local/share/fnm/node-versions"),
            PathBuf::from("installation/bin"),
        ),
        (
            home.join("Library/Application Support/fnm/node-versions"),
            PathBuf::from("installation/bin"),
        ),
    ];
    if let Some(nvm_dir) = env::var_os("NVM_DIR").filter(|value| !value.is_empty()) {
        roots.push((
            PathBuf::from(nvm_dir).join("versions/node"),
            PathBuf::from("bin"),
        ));
    }
    if let Some(fnm_dir) = env::var_os("FNM_DIR").filter(|value| !value.is_empty()) {
        roots.push((
            PathBuf::from(fnm_dir).join("node-versions"),
            PathBuf::from("installation/bin"),
        ));
    }
    if let Some(mise_dir) = env::var_os("MISE_DATA_DIR").filter(|value| !value.is_empty()) {
        roots.push((
            PathBuf::from(mise_dir).join("installs/node"),
            PathBuf::from("bin"),
        ));
    }

    for (root, suffix) in roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let candidate = entry.path().join(&suffix);
                if candidate.is_dir() {
                    push_unique_path(&mut paths, candidate);
                }
            }
        }
    }

    let fnm_multishells = home.join(".local/state/fnm_multishells");
    if let Ok(entries) = std::fs::read_dir(fnm_multishells) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("bin");
            if candidate.is_dir() {
                push_unique_path(&mut paths, candidate);
            }
        }
    }
    paths
}

fn extend_environment_search_paths(paths: &mut Vec<PathBuf>, platform: HostPlatform) {
    if let Some(prefix) = env::var_os("NPM_CONFIG_PREFIX").filter(|value| !value.is_empty()) {
        push_unique_path(
            paths,
            global_npm_bin_directory(PathBuf::from(prefix), platform),
        );
    }

    for variable in [
        "PNPM_HOME",
        "NVM_HOME",
        "NVM_SYMLINK",
        "OPENCODE_INSTALL_DIR",
        "XDG_BIN_DIR",
    ] {
        if let Some(path) = env::var_os(variable).filter(|value| !value.is_empty()) {
            push_unique_path(paths, PathBuf::from(path));
        }
    }
    for variable in ["VOLTA_HOME"] {
        if let Some(path) = env::var_os(variable).filter(|value| !value.is_empty()) {
            push_unique_path(paths, PathBuf::from(path).join("bin"));
        }
    }
    for variable in ["MISE_DATA_DIR", "ASDF_DATA_DIR"] {
        if let Some(path) = env::var_os(variable).filter(|value| !value.is_empty()) {
            push_unique_path(paths, PathBuf::from(path).join("shims"));
        }
    }
    for variable in ["SCOOP", "SCOOP_GLOBAL"] {
        if let Some(path) = env::var_os(variable).filter(|value| !value.is_empty()) {
            push_unique_path(paths, PathBuf::from(path).join("shims"));
        }
    }
    if let Some(gopath) = env::var_os("GOPATH").filter(|value| !value.is_empty()) {
        for path in env::split_paths(&gopath) {
            push_unique_path(paths, path.join("bin"));
        }
    }

    if platform == HostPlatform::Windows {
        if let Some(nvm_home) = env::var_os("NVM_HOME").filter(|value| !value.is_empty()) {
            if let Ok(entries) = std::fs::read_dir(PathBuf::from(nvm_home)) {
                for entry in entries.flatten() {
                    let candidate = entry.path();
                    if candidate.is_dir() {
                        push_unique_path(paths, candidate);
                    }
                }
            }
        }
    }
}

fn executable_candidates(command: &str, directory: &Path, platform: HostPlatform) -> Vec<PathBuf> {
    executable_names(command, platform)
        .into_iter()
        .map(|name| directory.join(name))
        .collect()
}

fn executable_names(command: &str, platform: HostPlatform) -> Vec<OsString> {
    if platform == HostPlatform::Windows {
        [".exe", ".cmd", ".bat", ""]
            .into_iter()
            .map(|extension| OsString::from(format!("{command}{extension}")))
            .collect()
    } else {
        vec![OsString::from(command)]
    }
}

fn inspect_executable(
    path: &Path,
    search_path: &OsString,
) -> (Option<String>, bool, Option<String>) {
    let mut command = Command::new(path);
    command.arg("--version").env("PATH", search_path);
    let output = run_command(command, VERSION_PROBE_TIMEOUT);

    match output {
        Ok(CommandRun::Completed(output)) if output.status.success() => {
            let stdout = decode_output(&output.stdout);
            let stderr = decode_output(&output.stderr);
            let raw = if stdout.trim().is_empty() {
                stderr
            } else {
                stdout
            };
            (Some(extract_version(&raw)), true, None)
        }
        Ok(CommandRun::Completed(output)) => {
            let detail = output_detail(&output);
            (None, false, (!detail.is_empty()).then_some(detail))
        }
        Ok(CommandRun::TimedOut { stdout, stderr }) => (
            None,
            false,
            Some(timeout_message(
                "Version check",
                VERSION_PROBE_TIMEOUT,
                &output_detail_from_bytes(&stdout, &stderr),
            )),
        ),
        Err(error) => (None, false, Some(error.to_string())),
    }
}

fn decode_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn output_detail(output: &Output) -> String {
    output_detail_from_bytes(&output.stdout, &output.stderr)
}

fn output_detail_from_bytes(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = decode_output(stderr);
    let stdout = decode_output(stdout);
    let output = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let redacted = redact_output(&output);
    last_lines(redacted.trim(), 8)
}

fn redact_output(output: &str) -> String {
    output
        .lines()
        .map(redact_output_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_output_line(line: &str) -> String {
    let normalized = line.to_ascii_lowercase();
    if [
        "_auth",
        "authorization",
        "bearer ",
        "npm_token",
        "token=",
        "password=",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        return "[redacted package-manager output]".to_string();
    }

    for scheme in ["https://", "http://"] {
        if let Some(start) = line.find(scheme) {
            let authority_start = start + scheme.len();
            let authority = &line[authority_start..];
            let authority_end = authority.find('/').unwrap_or(authority.len());
            if let Some(at) = authority[..authority_end].find('@') {
                return format!("{}***@{}", &line[..authority_start], &authority[at + 1..]);
            }
        }
    }
    line.to_string()
}

fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(count);
    lines[start..].join("\n")
}

fn extract_version(output: &str) -> String {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("unknown");
    let token = line
        .split_whitespace()
        .find(|token| {
            token
                .trim_start_matches('v')
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        })
        .unwrap_or(line);
    token.trim_start_matches('v').to_string()
}

fn infer_install_source(path: &Path, real_path: &Path) -> InstallationSource {
    let text = format!("{} {}", path.display(), real_path.display())
        .replace('\\', "/")
        .to_ascii_lowercase();

    if text.contains("/.nvm/") {
        InstallationSource::Nvm
    } else if text.contains("fnm_multishells") || text.contains("/.fnm/") || text.contains("/fnm/")
    {
        InstallationSource::Fnm
    } else if text.contains("/.mise/") || text.contains("/share/mise/") || text.contains("/mise/") {
        InstallationSource::Mise
    } else if text.contains("/homebrew/")
        || text.contains("/linuxbrew/")
        || text.contains("/cellar/")
    {
        InstallationSource::Homebrew
    } else if text.contains("/.local/share/claude/")
        || text.contains("/claude/versions/")
        || text.contains("/.opencode/")
        || text.contains("/programs/openai/codex/")
        || text.contains("/programs/claude/")
    {
        InstallationSource::Native
    } else if text.contains("/.local/share/pnpm/") || text.contains("/pnpm/") {
        InstallationSource::Pnpm
    } else if text.contains("/.volta/") || text.contains("/volta/") {
        InstallationSource::Volta
    } else if text.contains("/.bun/") {
        InstallationSource::Bun
    } else if text.contains("/scoop/") {
        InstallationSource::Scoop
    } else if text.contains("/node_modules/")
        || text.contains("/.npm-global/")
        || text.contains("/appdata/roaming/npm/")
        || text.contains("/appdata/local/npm/")
    {
        InstallationSource::Npm
    } else {
        InstallationSource::System
    }
}

fn package_manager_global_bin(run: &PackageManagerRun) -> Option<PathBuf> {
    let mut command = Command::new(&run.executable);
    match run.manager {
        "npm" => command.args(["prefix", "--global"]),
        "pnpm" => command.args(["bin", "--global"]),
        _ => return None,
    };
    command.env("PATH", &run.search_path);
    let output = match run_command(command, VERSION_PROBE_TIMEOUT).ok()? {
        CommandRun::Completed(output) => output,
        CommandRun::TimedOut { .. } => return None,
    };
    if !output.status.success() {
        return None;
    }

    let stdout = decode_output(&output.stdout);
    let directory = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let directory = PathBuf::from(directory);
    if run.manager == "npm" {
        Some(global_npm_bin_directory(directory, HostPlatform::current()))
    } else {
        Some(directory)
    }
}

fn global_npm_bin_directory(prefix: PathBuf, platform: HostPlatform) -> PathBuf {
    if platform == HostPlatform::Windows {
        prefix
    } else {
        prefix.join("bin")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        binary_search_paths_for, executable_names, extract_version, global_npm_bin_directory,
        infer_install_source, read_limited, redact_output, HostPlatform, InstallationSource,
        MAX_CAPTURED_OUTPUT_BYTES,
    };
    #[cfg(not(target_os = "windows"))]
    use super::{default_flag_for_shell, is_valid_shell, login_shell_path};
    #[cfg(windows)]
    use super::{package_manager_global_bin, paths_match, run_package_manager, PackageManagerRun};
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    #[test]
    fn includes_platform_specific_binary_locations() {
        let home = Path::new("/home/accly");
        let mac_paths =
            binary_search_paths_for(home, HostPlatform::Macos, &[], &[], None, None, None);
        assert!(mac_paths.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(mac_paths.contains(&home.join("Library/pnpm")));
        assert!(mac_paths.contains(&home.join(".opencode/bin")));

        let linux_paths =
            binary_search_paths_for(home, HostPlatform::Linux, &[], &[], None, None, None);
        assert!(linux_paths.contains(&PathBuf::from("/home/linuxbrew/.linuxbrew/bin")));
        assert!(linux_paths.contains(&home.join(".local/share/pnpm")));
        assert!(linux_paths.contains(&home.join("go/bin")));

        let windows_paths = binary_search_paths_for(
            Path::new(r"C:\Users\Accly"),
            HostPlatform::Windows,
            &[],
            &[],
            Some(Path::new(r"C:\Users\Accly\AppData\Roaming")),
            Some(Path::new(r"C:\Users\Accly\AppData\Local")),
            Some(Path::new(r"C:\Program Files")),
        );
        let windows_paths = windows_paths
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();
        assert!(windows_paths.contains(&"C:/Users/Accly/AppData/Roaming/npm".to_string()));
        assert!(windows_paths
            .contains(&"C:/Users/Accly/AppData/Local/Programs/OpenAI/Codex/bin".to_string()));
        assert!(windows_paths.contains(&"C:/Users/Accly/AppData/Local/Programs/claude".to_string()));
        assert!(windows_paths.contains(&"C:/Users/Accly/scoop/shims".to_string()));
    }

    #[test]
    fn selects_windows_executable_extensions() {
        assert_eq!(
            executable_names("codex", HostPlatform::Windows),
            ["codex.exe", "codex.cmd", "codex.bat", "codex"]
                .map(std::ffi::OsString::from)
                .to_vec()
        );
        assert_eq!(
            executable_names("codex", HostPlatform::Linux),
            vec![std::ffi::OsString::from("codex")]
        );
    }

    #[test]
    fn recognizes_package_manager_sources() {
        assert_eq!(
            infer_install_source(
                Path::new("/opt/homebrew/bin/codex"),
                Path::new("/opt/homebrew/Cellar/codex/1.0/bin/codex")
            ),
            InstallationSource::Homebrew
        );
        assert_eq!(
            infer_install_source(
                Path::new(r"C:\Users\Accly\AppData\Roaming\npm\codex.cmd"),
                Path::new(
                    r"C:\Users\Accly\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js"
                )
            ),
            InstallationSource::Npm
        );
        assert_eq!(
            infer_install_source(
                Path::new("/home/accly/.local/share/pnpm/opencode"),
                Path::new(
                    "/home/accly/.local/share/pnpm/global/5/node_modules/opencode-ai/bin/opencode"
                )
            ),
            InstallationSource::Pnpm
        );
        assert_eq!(
            infer_install_source(
                Path::new("/home/accly/.nvm/versions/node/v22/bin/codex"),
                Path::new("/home/accly/.nvm/versions/node/v22/bin/codex"),
            ),
            InstallationSource::Nvm
        );
        assert_eq!(
            infer_install_source(
                Path::new("/home/accly/.local/state/fnm_multishells/123/bin/codex"),
                Path::new("/home/accly/.local/state/fnm_multishells/123/bin/codex"),
            ),
            InstallationSource::Fnm
        );
        assert_eq!(
            infer_install_source(
                Path::new("/home/accly/.local/share/mise/installs/node/22/bin/codex"),
                Path::new("/home/accly/.local/share/mise/installs/node/22/bin/codex"),
            ),
            InstallationSource::Mise
        );
        assert_eq!(
            infer_install_source(
                Path::new(r"C:\Users\Accly\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe"),
                Path::new(r"C:\Users\Accly\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe"),
            ),
            InstallationSource::Native
        );
    }

    #[test]
    fn extracts_a_compact_version_from_cli_output() {
        assert_eq!(extract_version("Codex CLI 0.99.0\n"), "0.99.0");
        assert_eq!(extract_version("v2.3.4"), "2.3.4");
    }

    #[test]
    fn resolves_global_npm_bin_per_platform() {
        assert_eq!(
            global_npm_bin_directory(PathBuf::from("/opt/homebrew"), HostPlatform::Macos),
            PathBuf::from("/opt/homebrew/bin")
        );
        assert_eq!(
            global_npm_bin_directory(
                PathBuf::from(r"C:\\Users\\Accly\\AppData\\Roaming\\npm"),
                HostPlatform::Windows
            ),
            PathBuf::from(r"C:\\Users\\Accly\\AppData\\Roaming\\npm")
        );
    }

    #[test]
    fn redacts_package_manager_credentials_from_errors() {
        let output = redact_output(
            "npm ERR! authorization: Bearer top-secret\nhttps://user:password@registry.example/npm",
        );

        assert!(!output.contains("top-secret"));
        assert!(!output.contains("user:password"));
        assert!(output.contains("https://***@registry.example/npm"));
    }

    #[test]
    fn caps_captured_command_output() {
        let input = vec![b'x'; MAX_CAPTURED_OUTPUT_BYTES + 32];
        let captured = read_limited(Cursor::new(input));

        assert_eq!(captured.len(), MAX_CAPTURED_OUTPUT_BYTES);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn validates_login_shell_candidates() {
        assert!(is_valid_shell("/bin/zsh"));
        assert!(is_valid_shell("bash"));
        assert!(!is_valid_shell("powershell"));
        assert_eq!(default_flag_for_shell("/bin/zsh"), "-lic");
        assert_eq!(default_flag_for_shell("/bin/sh"), "-c");
        let _ = login_shell_path();
    }

    #[cfg(windows)]
    #[test]
    fn executes_windows_command_shims() {
        use std::env;
        use std::fs;
        use tempfile::tempdir;

        let directory = tempdir().unwrap();
        let search_path = env::join_paths([directory.path()]).unwrap();
        let codex = directory.path().join("codex.cmd");
        fs::write(
            &codex,
            "@echo off\r\nif \"%1\"==\"--version\" echo codex 1.2.3\r\n",
        )
        .unwrap();
        let (version, runnable, detail) = super::inspect_executable(&codex, &search_path);
        assert!(runnable, "{detail:?}");
        assert_eq!(version.as_deref(), Some("1.2.3"));

        let npm = directory.path().join("npm.cmd");
        fs::write(
            &npm,
            "@echo off\r\nif \"%1\"==\"prefix\" (\r\n  echo %~dp0\r\n  exit /b 0\r\n)\r\nexit /b 0\r\n",
        )
        .unwrap();
        let run = PackageManagerRun {
            executable: npm,
            manager: "npm",
            search_path,
        };
        let global_bin = package_manager_global_bin(&run).unwrap();
        assert!(paths_match(&global_bin, directory.path()));
        run_package_manager(&run, "@openai/codex").unwrap();
    }
}
