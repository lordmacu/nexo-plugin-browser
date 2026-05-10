# Changelog

All notable changes to `nexo-plugin-browser` are documented here.
The project adheres to [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added

- **Cross-platform Chrome / Chromium / Edge auto-detect**
  (Phase 27.x.browser-windows-discovery). Out-of-the-box
  install paths now resolve on every host:
  - **Windows**: `%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe`,
    `%LOCALAPPDATA%\Microsoft\Edge\Application\msedge.exe`,
    `%LOCALAPPDATA%\Chromium\Application\chrome.exe`,
    `%ProgramFiles%\Google\Chrome\Application\chrome.exe`,
    `%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe`,
    `%ProgramFiles%\Microsoft\Edge\Application\msedge.exe`,
    `%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe`,
    plus `chrome` / `msedge` via `PATH` (PATHEXT-aware).
  - **macOS**: `/Applications/Google Chrome.app`,
    `/Applications/Chromium.app`,
    `/Applications/Microsoft Edge.app`, plus
    `$HOME/Applications/...` per-user copies, plus Homebrew
    Cask `google-chrome` / `chromium` PATH shims.
  - **Linux**: `/usr/bin/google-chrome`,
    `/usr/bin/google-chrome-stable`,
    `/usr/bin/chromium-browser`, `/usr/bin/chromium`,
    `/snap/bin/chromium`,
    `/data/data/com.termux/files/usr/bin/chromium` (Termux
    Android), plus the same names via `PATH`.
- **Edge as a first-class auto-detect target.** Microsoft Edge
  is Chromium-based; CDP works against `msedge.exe` identically.
  Windows enterprise installs that ship Edge but not Chrome
  now resolve a usable browser without the explicit
  `NEXO_PLUGIN_BROWSER_EXECUTABLE` override.
- **`browser.discovery` tracing log** at `info` level on every
  launch. Structured fields `kind` (`Chrome` / `Chromium` /
  `Edge` / `Custom`), `path`, and `source` (`auto-detect` or
  `env-override`) — grep-friendly diagnostic for "why did the
  daemon pick *that* browser" reports.
- **Searched-paths list in the not-found error.** When
  auto-detect exhausts every Tier 1 (bundled) and Tier 2
  (`PATH`) candidate, the failure message now enumerates each
  concrete probe site so the operator can paste the list
  straight into a bug report or set
  `NEXO_PLUGIN_BROWSER_EXECUTABLE` against an absolute path
  that exists.

### Changed

- **Windows `PATH` lookup honours the platform separator and
  `PATHEXT`.** The pre-existing hand-rolled `which_exists`
  split `PATH` by `:` unconditionally, which broke Windows
  (separator is `;`) and truncated drive letters
  (`C:\Program Files\...` became `["C", "\Program Files\..."]`).
  Discovery now routes bare names through the `which` crate.
- **`NEXO_PLUGIN_BROWSER_EXECUTABLE` fail-fast on missing
  path.** A typo in the override (or a stale path that's been
  uninstalled) used to silently fall through to the spawn
  step where the operator saw a generic
  `failed to spawn Chrome (...): No such file or directory`.
  The plugin now stat-s the override up front and surfaces
  `NEXO_PLUGIN_BROWSER_EXECUTABLE points to non-existent path:
  <path>` so the operator notices the typo immediately.
  **Behaviour change** — operators who relied on the silent
  fallback need to either fix the override or unset it to
  re-engage auto-detect.

### Tests

- 17 new unit tests: per-OS pure builders
  (`linux_candidates`, `macos_candidates`,
  `windows_candidates`) exercise candidate shape, ordering
  (Chrome ranks before Edge before Chromium, system before
  user), and env-var fallback. `bundled_candidates` /
  `path_lookup_names` host-dispatch smoke. Override fail-fast
  e2e (`launch_fails_fast_when_override_path_missing`).
- New `.github/workflows/ci.yml` — `cargo nextest run --lib`
  on `ubuntu-latest`, `macos-14`, `windows-latest` runners
  every push + PR. Per-platform discovery cfg-branches stay
  green.

### Internal

- New `which = "7"` dep replaces the hand-rolled `which_exists`
  PATH walker. Honours `PATHEXT` (Windows `chrome` resolves to
  `chrome.exe`), platform separator, and registered
  shortcuts.
- Discovery extracted into `src/chrome/discovery.rs` with
  per-OS bundled-candidate builders + a single
  `find_chrome_executable() -> Result<BrowserExecutable,
  DiscoveryError>` orchestrator. Pure builders carry no `cfg`
  guard so a Linux dev box can validate Windows / macOS
  candidate shapes without spinning up a runner of that OS.

## [0.2.1] – 2026-05-07

### Added

- **Per-agent Chrome profile isolation** (Phase 81.17.c.multi-profile).
  The subprocess now holds a `DashMap<agent_id, BrowserPlugin>`
  keyed on the sanitised `tool.invoke.agent_id` — first call
  per agent lazy-boots a Chrome with
  `${BASE}/profiles/<agent_id>/`. Cookies, localStorage, and
  login state stay isolated between agents without per-agent
  YAML configuration.
- New env knobs:
  - `NEXO_PLUGIN_BROWSER_MAX_PROFILES` (default `10`, range
    `[1, 64]`) — cap on simultaneous active profiles. Cap
    reached → `tool.invoke` returns `-33404 Unavailable`.
  - `NEXO_PLUGIN_BROWSER_PROFILE_IDLE_SECS` (default `900`,
    range `[0, 86400]`) — idle threshold for Chrome eviction.
    `0` disables. The on-disk profile dir is preserved across
    eviction so the next call lazy-reboots with the same
    cookies / login state.
  - `NEXO_PLUGIN_BROWSER_MULTI_PROFILE` (default `true`) —
    opt-out for legacy single-shared-profile mode (v0.2.0
    behaviour).
- Chrome profile chip decoration: each per-agent Chrome window
  displays the agent's `agent_id` + a stable color derived from
  `sha256(agent_id)[..3]` so operators eyeballing N parallel
  Chromes can tell them apart.
- Sanitiser for `agent_id`: must match `^[A-Za-z0-9_-]{1,64}$`
  after ASCII-lowercasing. Path-traversal / control chars /
  Unicode punctuation rejected with `-33402 ArgumentInvalid`.

### Changed

- Background idle eviction loop (every 30 s) auto-closes
  Chrome processes whose `last_active_at` exceeds the idle
  threshold. `last_active_at` is updated only on successful
  tool calls — failures preserve the idle clock.

### Tests

- 5 new e2e tests in `tests/e2e_multi_profile.rs` (distinct
  agents, default fallback, cap, sanitiser rejection, opt-out
  routing) — none require a Chromium binary.
- 8 new unit tests in `src/profile_limits.rs`.
- 17 new unit tests in `src/profile.rs`.
- 3 new unit tests in `src/profile_decoration.rs`.
- Existing handshake + persistence e2e tests (5) still pass.

## [0.2.0] – 2026-05-07

### Changed

- **Slim refactor: 4 deps from crates.io alone** — dropped
  `nexo-core` / `nexo-llm` / `nexo-resilience` /
  `nexo-plugin-manifest` direct deps. The plugin now depends
  only on `nexo-microapp-sdk = "0.1.2"`, `nexo-broker = "0.1.2"`,
  `nexo-cdp = "0.1.0"`, `nexo-config = "0.1.4"`. External
  operators install via `cargo install nexo-plugin-browser`
  with no proyecto sibling checkout required.
- `BrowserPlugin` rewritten (~190 LOC, was 490): drops the
  legacy in-tree `Plugin` / `NexoPlugin` trait impls,
  CircuitBreaker, broker bridge, cached_manifest. Subprocess
  flow handles those concerns at the `PluginAdapter` layer.
- `tool.rs` deleted; per-tool `ToolDef` instances inlined in
  `tool_defs.rs` (~250 LOC).

### Added

- Initial extraction from `nexo-rs/proyecto/crates/plugins/browser/`
  to a standalone repo (Phase 81.17.c).
- Subprocess plugin contract v1.10.0 conformance:
  `initialize` reply advertises 12 `browser_*` tools;
  `tool.invoke` round-trips per tool.
- Per-agent CDP session lifecycle (lazy-boot Chrome, multi-tool
  reuse, explicit shutdown).
- Sandbox + supervisor manifest sections (Phase 81.21 +
  81.22).
- 18 lib tests + 3 e2e_handshake (+1 ignored requiring
  Chromium) + 2 e2e_persistence + 3 latency-bench
  scenarios.

[Unreleased]: https://github.com/lordmacu/nexo-plugin-browser/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/lordmacu/nexo-plugin-browser/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/lordmacu/nexo-plugin-browser/releases/tag/v0.2.0
