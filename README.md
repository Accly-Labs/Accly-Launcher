# Accly Launcher

Accly Launcher is a macOS-first Tauri desktop app for connecting a paid Accly
account to supported local AI agents. It configures agent settings only; it
does not install, bundle, proxy, or execute any agent.

## Local development

```sh
pnpm install
pnpm dev
```

Use `pnpm dev:web` for renderer-only work. The browser fallback has sample data
and never writes local agent configuration.

Public endpoint configuration is optional during local UI work. For native
builds, set `ACCLY_AUTH_URL`, `ACCLY_CORE_URL`, and
`ACCLY_LAUNCHER_CLIENT_ID` in the build environment. `VITE_ACCLY_GATEWAY_URL`
is bundled into the renderer for agent configuration. See `.env.example`.

## Architecture

- React 19, TypeScript, Tailwind, TanStack Query, and i18next render the
  three-step account-to-agent workflow.
- Native Rust owns all keychain, filesystem, process-detection, and HTTP work.
  The WebView does not receive the launcher session token.
- Each agent is a Rust `AgentAdapter`. Adapters prepare narrowly scoped changes;
  a shared `FileTransaction` backs up raw bytes, atomically writes, validates,
  and restores on failure.
- The launcher uses macOS Keychain for the Better Auth device session. Gateway
  API keys exist in renderer memory only long enough to configure the selected
  agent.

## Agent support

| Agent | Detection | Safe configuration |
| --- | --- | --- |
| Codex | CLI and config | `~/.codex/config.toml` and `auth.json` |
| Claude Code | CLI and config | `~/.claude/settings.json` |
| Gemini CLI | CLI and config | `~/.gemini/.env` and `settings.json` |
| OpenCode | CLI and config | `~/.config/opencode/opencode.json` |
| Cursor | app/config detection | Not enabled without a verified gateway contract |
| Windsurf | app/config detection | Not enabled without a verified gateway contract |

The launcher deliberately rejects non-strict JSON/JSONC targets instead of
rewriting them and potentially losing comments or unsupported syntax.

## Backend handoff

The UI and native client are ready for Better Auth Device Authorization, but
the current Accly services need these server-side changes before live login can
work:

1. Register Better Auth's device-authorization plugin in `accly-auth`, add its
   persistence schema, and ship the browser approval route.
2. Issue a launcher-scoped, revocable credential rather than forwarding a normal
   seven-day browser session token.
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
pnpm typecheck
pnpm build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
```
