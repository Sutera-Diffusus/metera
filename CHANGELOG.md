# Changelog

This file records user-visible changes. Internal build metadata remains
`1.7.0`; `1.7.0.1` is the user-facing Windows release label for the first
public release.

## 1.7.0.1 - 2026-08-14

First public GitHub release.

### Added

- Unified Windows dashboard for AI coding-tool usage and cost information.
- Acrylic floating meter and tray controls.
- Local SQLite usage history with configurable storage.
- Provider pricing, token, activity, and quota views.
- Optional daily email reports through a user-configured SMTP server.
- Support for Codex, Claude Code, Kimi Code, WorkBuddy, ZCode, Reasonix, and
  DeepSeek Harness data sources.

### Security and privacy

- Usage data is kept in the local application data directory unless the user
  explicitly enables an email report.
- Quota integrations read local authentication state and call provider
  services directly; Metera does not provide a hosted telemetry backend.

### Known limitations

- Windows is the supported desktop platform.
- Provider APIs and local credential formats can change independently of
  Metera.
- SMTP authorization data is currently stored in the local settings file.
- Release installers may be unsigned.
