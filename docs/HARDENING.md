# EduMind Hardening and Release Gates

## Safe defaults

The checked-in example configuration is safe for local development:

- The gateway binds to loopback and uses no-auth only on that loopback address.
  A non-loopback no-auth bind is rejected unless EDUMIND_ALLOW_INSECURE_BIND=1
  is set deliberately.
- Gateway request bodies, tool rounds, tool output, writes, timeouts, process
  memory, rate limits, audit history, and daily tool classes all have positive,
  explicit caps.
- The safe tool profile and explicit tool allow-list are enabled. Sensitive
  actions require an Argon2id action-password grant, and tool writes remain
  restricted to configured roots.
- Windows process launches use Job Objects when configured, while external
  downloads and mobile sync use HTTPS and bounded responses.

The config loader test verifies that these protections are written explicitly
in edumind/config.example.yaml rather than being supplied only by serde
defaults. If a deployment changes any cap, review the new bound, expected
workload, and recovery behavior before release.

## Release commands

Run these from the repository root unless a command changes directory:

~~~powershell
cargo fmt --check
cargo build --workspace
cargo run -p edumind -- --config edumind/config.example.yaml --check-config
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

Push-Location edumind-tauri
npm ci
npm run lint
npm test
npm run test:safety
npm run test:e2e
npm run build
Pop-Location

Push-Location mobile
.\gradlew.bat :app:assembleDebug :app:lintDebug
Pop-Location
~~~

The Android check needs a Java 17-compatible JDK and Android SDK Platform 34.
See mobile/README.md for local environment setup.

## CI coverage

The GitHub workflow runs the Rust format, build, example-config validation,
tests, and blocking clippy check. It also runs desktop lint, unit, Node safety,
Playwright browser, and production-build checks, then assembles and lints the
MeetMind Android app. CI never requires production secrets: mobile credentials
are runtime configuration and remain blank during compilation.

## Operating boundaries

Do not loosen an execution cap, add a write root, expose a gateway, or enable a
network integration as a convenience workaround. Make the narrowest change,
document its threat model, and add a test that proves the new boundary.
Never put credentials, raw transcripts, source documents, or generated student
artifacts in repository documentation, test fixtures, or CI logs.
