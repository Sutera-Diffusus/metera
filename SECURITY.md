# Security policy

## Scope

Metera is a local-first Windows application. It can read local usage files and
local authentication state for supported tools, call provider quota endpoints,
call an exchange-rate service, and connect to a user-configured SMTP server.
It does not require a Metera account or send usage history to a Metera-hosted
service.

The following local data may be sensitive:

- provider authentication files and OAuth tokens;
- usage databases and session logs;
- project and model names;
- `settings.json`, including SMTP configuration.

Do not attach any of these files, real tokens, API keys, or SMTP passwords to
a public issue or pull request.

## Reporting a vulnerability

Please report security issues privately through GitHub Security Advisories
once the repository is enabled for them, or use the private contact channel
listed in the maintainer's GitHub profile. Include the affected release,
operating system, reproduction steps, and the smallest possible sanitized
example.

Do not publicly disclose an issue before a fix or mitigation is available.

## Current security notes

- Credential values are intended to stay in the native process and must never
  be copied into logs, screenshots, tests, or error reports.
- SMTP authorization data is currently serialized in the local settings file.
  Use an application-specific password and protect the Windows user profile.
  A protected Windows credential-store implementation is a future hardening
  item.
- The Tauri application security configuration should be reviewed whenever
  frontend capabilities or remote content are added.
