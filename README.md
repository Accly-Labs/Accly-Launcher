# Accly Launcher

Accly Launcher is a native Tauri desktop app for connecting an Accly account
to local AI coding agents on macOS, Windows, and Linux. It discovers supported
agents, reports their local installation state, and safely configures the
selected agent to use Accly.

The WebView never receives the launcher session token. Native Rust owns
credential storage, local process inspection, agent lifecycle actions, file
writes, and HTTP requests.

## Release metadata

The current launcher version is `0.2.2`. The npm package, Cargo package, and
Tauri bundle metadata are kept in sync for every release. See
[CHANGELOG.md](CHANGELOG.md) for the shipped behavior and upgrade notes.

## Supported platforms

The native application supports macOS, Windows, and Linux. Windows support is
native; WSL is a separate environment and is not scanned or configured through
the Windows build.

On each supported operating system, the agent scanner searches the inherited
`PATH`, the user's login-shell `PATH`, and common locations for npm, nvm, fnm,
mise, pnpm, Volta, Bun, Homebrew/Linuxbrew, Scoop, and native installations.
It also checks fnm multishell directories and the standalone Windows Codex and
Claude installers. The scanner records the executable path, reported version,
inferred install source, and number of discovered installations. The first
discovered installation is shown as the primary one; an unavailable primary is
marked as broken.

Scanning happens when the app opens, when the user selects **Scan again**, and
every 30 seconds while the app is in the foreground. This is local process
inspection only; it does not poll Accly services or run a background daemon.
Each version probe has a five-second timeout, so a broken CLI cannot block the
workspace indefinitely.

## Agent support

| Agent       | Detection and lifecycle                          | Safe configuration                                                              |
| ----------- | ------------------------------------------------ | ------------------------------------------------------------------------------- |
| Codex       | Detect, install, update/repair, and rescan       | `~/.codex/config.toml` and `auth.json`                                          |
| Claude Code | Detect, install, update/repair, and rescan       | `~/.claude/settings.json`                                                       |
| Gemini CLI  | Detect, install, update/repair, and rescan       | `~/.gemini/.env` and `settings.json`                                            |
| OpenCode    | Detect, install, update/repair, and rescan       | `$XDG_CONFIG_HOME/opencode/opencode.json` or `.jsonc` (same under `~/.config`)       |
| Cursor      | App and settings detection only                  | Not enabled without a verified gateway contract                                 |
| Windsurf    | App and settings detection only                  | Not enabled without a verified gateway contract                                 |

`Install` is intentionally limited to an internal allowlist of official npm
packages:

| Agent       | Package                     |
| ----------- | --------------------------- |
| Codex       | `@openai/codex`             |
| Claude Code | `@anthropic-ai/claude-code` |
| Gemini CLI  | `@google/gemini-cli`        |
| OpenCode    | `opencode-ai`               |

An explicit user click runs the fixed native command
`npm install --global <allowlisted-package>@latest`. The launcher never accepts
a shell command or package name from the renderer, never installs Node.js for
the user, and never runs a remote script. Node.js and npm must already be
installed and discoverable to use **Install**.

`Update` is enabled only when the detected installation is managed by npm,
nvm, fnm, mise, or pnpm. Homebrew, Linuxbrew, Volta, Bun, Scoop, native
installers, and system packages remain source-owned: update them with their
original installer, then scan again. This avoids replacing a deliberately
managed installation with a second global npm copy. The launcher also verifies
that the selected npm/pnpm global bin directory matches the detected executable
before updating. Multiple installations disable automatic updates until the
user resolves the intended copy.

If an npm/pnpm-managed executable is present but cannot run, **Repair** repeats
the allowlisted package install against that same verified global directory.
Other broken installation sources stay source-owned and receive a manual
remediation message. The workspace shows checking, install/update/repair, and
validation phases, then immediately rescans to record the final version and
path. Package-manager commands have a two-minute timeout; captured failure
output is limited and redacts credential-shaped values before it reaches the
WebView.

Cursor and Windsurf are deliberately detection-only. The launcher does not
install or rewrite either editor until a stable, verified custom gateway
configuration contract exists.

## Configuration safety

Each configurable agent is implemented as a Rust `AgentAdapter`. A shared
`FileTransaction` backs up original bytes, rejects symlinked targets, writes
atomically, validates the result, and restores the previous configuration on
failure. Invalid JSON and JSONC targets are left unchanged rather than being
partially rewritten. OpenCode JSONC is parsed using its JSON5-compatible
syntax; the resulting file is written as valid JSON while preserving all
existing configuration values.

Gemini CLI uses the native Google Generative AI protocol. Its configured base
URL is the gateway origin without `/v1` or `/v1beta`; Gemini CLI supplies that
API version and model route itself. The gateway accepts Gemini CLI's standard
`X-Goog-Api-Key` header as well as the other documented Accly API-key headers.

### Gemini CLI gateway contract

When **Configure** is selected for Gemini CLI, the launcher writes these
managed values to `~/.gemini/.env` and selects `gemini-api-key` authentication
in `~/.gemini/settings.json`:

```dotenv
GOOGLE_GEMINI_BASE_URL=https://gateway.example.com
GEMINI_API_KEY=<Accly Google-compatible API key>
GEMINI_MODEL=<selected Gemini model>
```

`GOOGLE_GEMINI_BASE_URL` must be the gateway origin only. Do not append `/v1`
or `/v1beta`: Gemini CLI adds its own `/v1beta/models/...:generateContent`
route. The Accly Gateway must expose that native route and accept the standard
`X-Goog-Api-Key` request header.

Users upgrading from a launcher build that wrote a versioned base URL should
select **Change** and then **Configure** for Gemini CLI once. The launcher
rewrites the managed values to the native contract and creates a configuration
backup before changing either file. After a Gateway rollout, start a new
`gemini` process so it reads the updated environment.

The device session is stored through the operating system credential store:
macOS Keychain, Windows Credential Manager, or the Linux Secret Service. A
gateway API key exists in renderer memory only for the time needed to configure
the selected agent. Linux users need a Secret Service provider such as GNOME
Keyring or KWallet running in their desktop session; the launcher does not fall
back to plaintext token storage.

Auth is the source of truth for launcher-session validity. On startup, the
launcher verifies a stored bearer with Auth and removes it locally when Auth
reports it expired or revoked. Launcher sessions expire after seven days. A
running launcher schedules a fresh validation at the reported expiry, while an
unauthorized Core response also returns the user to sign-in. Device approval
codes expire after 15 minutes; the UI shows the remaining time, stops polling
at expiry, and lets the user explicitly generate a new code. It never silently
creates a new browser approval flow.

## Local development

```sh
pnpm install
pnpm dev
```

Use `pnpm dev:web` for renderer-only work. The browser fallback uses in-memory
sample data; it never runs an agent install/update command or writes local
agent configuration.

Public endpoint configuration is optional during local UI work. For native
builds, set `ACCLY_AUTH_URL`, `ACCLY_CORE_URL`, and
`ACCLY_LAUNCHER_CLIENT_ID` in the build environment.
`VITE_ACCLY_GATEWAY_URL` is bundled into the renderer for agent configuration.
See `.env.example`.

For a local native bundle, include the `local` feature and point all services at
localhost:

```sh
ACCLY_AUTH_URL=http://localhost:3003 \
ACCLY_CORE_URL=http://localhost:3001 \
ACCLY_LAUNCHER_CLIENT_ID=accly-launcher \
VITE_ACCLY_GATEWAY_URL=http://localhost:8080 \
pnpm tauri build --debug --features local
```

## Backend handoff

The UI and native client are ready for Better Auth Device Authorization, but
the current Accly services need these server-side changes before live login can
work:

1. Register Better Auth's device-authorization plugin in `accly-auth`, add its
   persistence schema, and ship the browser approval route.
2. Issue a launcher-scoped, revocable credential rather than forwarding a
   normal seven-day browser session token.
3. Let `accly_app` validate that credential on launcher routes. Current core
   middleware accepts signed cookies only; do not put `BETTER_AUTH_SECRET` or an
   internal service token in the launcher.
4. Expose an atomic `POST /api/v1/api-keys/:prefix/regenerate` endpoint. The
   launcher will not create-then-delete because that fails at the 10-key limit
   and is not atomic.
5. Expose a launcher-safe model catalog with plan and agent compatibility. Do
   not expose the internal `/api/v1/models*` token to a desktop client.

The native client is intentionally limited to public auth/core URLs so browser
CORS settings do not become part of the desktop trust boundary.

## Updates

The launcher checks the Accly Launcher GitHub release feed while open. Applying
an update is intentionally deferred until the release pipeline provides a
signed Tauri updater manifest and public key; downloading arbitrary GitHub
assets directly would bypass Tauri's update verification model.

## Validation

```sh
pnpm format:check
pnpm typecheck
pnpm build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

GitHub Actions runs the frontend quality checks on Ubuntu and the native Rust
test suite on macOS, Windows, and Linux. The matrix catches platform-specific
credential-store and agent-runtime build regressions before release packaging.
