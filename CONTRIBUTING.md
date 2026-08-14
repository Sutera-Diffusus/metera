# Contributing to Metera

Thanks for helping improve Metera. The project currently targets Windows and
uses a React/Vite frontend with a Rust/Tauri desktop shell.

## Development requirements

- Windows 10 or later
- Node.js 20 or later
- pnpm 11
- Rust 1.88.0, selected by `rust-toolchain.toml`
- Tauri 2 Windows build prerequisites and WebView2

Install dependencies and run the checks:

```powershell
pnpm install --frozen-lockfile
pnpm test
pnpm build
```

Run the desktop application with:

```powershell
pnpm tauri dev
```

Use a throwaway `METERA_DATA_DIR` when testing scanners, settings, or email
reports. Never run tests with personal credentials when a fixture or mocked
environment is sufficient.

## Pull requests

- Keep changes focused and explain the user impact.
- Add or update tests for behavior changes.
- Do not include credentials, local databases, browser profiles, installers,
  screenshots containing personal data, or machine-specific absolute paths.
- Describe any provider API, data-access, or security implications.
- Use clear commit messages such as `feat:`, `fix:`, `test:`, `docs:`, or
  `chore:`.

Before opening a pull request, run `pnpm test` and `pnpm build`. Changes to
the Tauri shell should also be checked with `pnpm tauri build` when the local
Windows toolchain is available.
