# Metera

[简体中文](README.zh-CN.md) · English

> A local-first Windows desktop usage meter for AI coding tools.

Metera is a Tauri desktop application that brings usage, token, cost, quota,
and activity information from several AI coding tools into one dashboard. It
also provides an acrylic floating meter, tray controls, local SQLite storage,
auditable provider pricing, and optional daily email reports.

![Metera launch banner](docs/assets/metera-launch-banner.png)

The current user-facing release is **1.7.0.1**. The package manifests keep the
build version at `1.7.0` because Cargo, npm, and Tauri use three-component
Semantic Versioning internally.

## Features

- Dashboard with usage, cost, token, activity, and provider views
- Acrylic floating meter and Windows tray controls
- Local SQLite history and configurable data directory
- Provider pricing and cost estimation
- Quota views for supported providers when local credentials are available
- Optional daily email reports through the user's SMTP server
- No Metera account, hosted backend, or usage-data telemetry

## Supported sources

Metera currently integrates with local data or APIs associated with:

- Codex
- Claude Code
- Kimi Code
- WorkBuddy
- ZCode
- Reasonix
- DeepSeek Harness (DSH)

Provider file formats, quota APIs, authentication formats, and pricing can
change without notice. A provider name or logo in this project does not imply
an endorsement, sponsorship, or official affiliation.

## Privacy and network behavior

Metera is local-first, but it is not completely offline:

- Usage history is stored locally in SQLite.
- The app reads local usage files and, for quota features, local credential
  files such as Codex `auth.json`, Kimi credentials, or DSH credentials.
- Quota and exchange-rate features make outbound requests to the relevant
  provider services and the configured exchange-rate service.
- Daily reports connect directly to the SMTP server configured by the user.
- Metera does not operate a server that receives usage history and does not
  include a general analytics or telemetry service.

Project names, model names, token counts, and email report contents can be
sensitive. Do not share `settings.json`, the local database, credential files,
or unsanitized screenshots when reporting a problem.

## Installation

Download the Windows x64 installer from [GitHub Releases](../../releases). The
installer uses a current-user install
and may download WebView2 through the Tauri bootstrapper if WebView2 is not
already installed.

The first public installer may not be code-signed. Windows SmartScreen can
therefore show an additional warning; verify the release checksum and source
before installing.

## Data directory

Set `METERA_DATA_DIR` before launch to choose the runtime data directory:

```powershell
$env:METERA_DATA_DIR = "D:\MeteraData-test"
```

For compatibility with existing installations, an existing `D:\MeteraData`
directory is preferred. On a new machine, Metera falls back to the standard
Windows user-data directory returned by the platform. The application does
not require a D: drive on a clean installation.

## Development

Development is currently Windows-only. Install Node.js, pnpm, Rust 1.88.0,
and the Windows build tools required by Tauri 2. WebView2 is required to run
the desktop application.

```powershell
pnpm install --frozen-lockfile
pnpm test
pnpm build
pnpm tauri dev
```

To create the Windows installer:

```powershell
pnpm tauri build
```

The NSIS installer is written to `target\release\bundle\nsis`. Do not commit
that directory; upload verified installers as GitHub Release assets instead.

## Known limitations

- Windows is the supported platform for the desktop application.
- Provider integrations depend on local file layouts and provider APIs that
  may change.
- Quota data is only available when the relevant local authentication state is
  valid and accessible.
- SMTP authorization data is currently stored in the local settings file.
  Use an application-specific password and protect the local profile.
- Release installers may be unsigned until code signing is configured.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for local setup, validation commands,
and pull request expectations. Please never include real credentials, usage
databases, or personal diagnostic output in a commit.

## License

Metera is released under the [MIT License](LICENSE). Third-party dependencies,
provider names, and provider logos remain subject to their respective terms.
