# Changelog

All notable user-facing changes to Accly Launcher are documented here.

## 0.2.0 - 2026-07-14

### Added

- Native cross-platform discovery for Codex, Claude Code, Gemini CLI, and
  OpenCode across macOS, Windows, and Linux.
- Installation, verified npm/pnpm update, repair, progress reporting, and
  post-action rescan for the supported CLI agents.
- Safe configuration writes with backups, atomic validation, and rollback for
  supported agent configuration files.
- Platform-aware credential-store support for macOS Keychain, Windows
  Credential Manager, and Linux Secret Service.
- GitHub Actions checks for frontend quality and native tests on macOS,
  Windows, and Linux.

### Changed

- The desktop workspace now presents local agent status, installation source,
  executable path, version, and lifecycle progress without a persistent
  sidebar.
- Agent discovery refreshes locally while the app is foregrounded; it does not
  create recurring backend load.
- Application metadata is synchronized at version `0.2.0` across npm, Cargo,
  and Tauri bundle configuration.

### Safety and compatibility

- Only fixed, official package names can be installed. The renderer cannot
  provide an arbitrary command or package name.
- Automatic updates are limited to unambiguous npm/pnpm installations whose
  global binary directory matches the detected executable. Other package
  sources remain managed by their original installer.
- Cursor and Windsurf remain detection-only until their Accly gateway
  configuration contract is verified.
- Windows support is native. WSL remains a separate environment.
