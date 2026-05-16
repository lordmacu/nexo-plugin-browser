# Changelog

All notable changes to `nexo-plugin-browser` are documented here.
The project adheres to [Semantic Versioning](https://semver.org).

## [0.4.0] — 2026-05-16

### Added

- **Tier 0 auto-download discovery (opt-in).** When the
  `auto-download` cargo feature is on **and** the runtime env
  var `NEXO_PLUGIN_BROWSER_AUTO_DOWNLOAD=1` is set, the plugin
  downloads Google's officially-published
  `chrome-headless-shell` for the running platform on first
  launch and caches it under
  `$XDG_CACHE_HOME/nexo-plugin-browser/chrome-for-testing/<version>/`.
  Soft-fails back to system discovery on network outage so a
  flaky first run doesn't block the plugin if a system Chrome
  is on `$PATH`.

  Both gates intentional:
    - cargo feature controls whether the dep compiles in at
      all (default off — the published plugin tarball stays
      slim).
    - env var lets operators flip behaviour at runtime
      without rebuilding.

  Cross-platform by construction — same path resolves
  linux64 / mac-x64 / mac-arm64 / win64 / win32. Pulled
  from Google's `chrome-for-testing-public` CDN over the
  catalogue at
  `https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json`.

  Cache layout:
  ```
  $XDG_CACHE_HOME/nexo-plugin-browser/chrome-for-testing/
  └── <version>/                                e.g. 148.0.7778.167
      └── chrome-headless-shell-<platform>/
          └── chrome-headless-shell             (binary; .exe on Windows)
  ```

  Override the cache root with `NEXO_BROWSER_CACHE=/some/path`.

### Internal

- Discovery refactored from a two-tier (auto-detect +
  override) into a three-tier shape (Tier 0 auto-download,
  Tier 1+2 auto-detect, override). `ChromeLauncher::launch`
  branches via the new `try_auto_download()` helper before
  falling back to `find_chrome_executable()`. Cargo-feature-
  gated so the default build is byte-identical to 0.3.x.

- New module `src/auto_download/` (vendored, ~200 LOC: a
  `Platform` enum, a `Manager` that fetches the JSON
  catalogue, downloads + extracts the zip, chmods the
  binary). No external crate dep — the logic lives inside
  the plugin so crates.io publish stays self-contained.

- New integration test `tests/auto_download.rs` (gated on
  `NEXO_BROWSER_LIVE_TESTS=1`, `--features auto-download`)
  exercises the full Tier 0 → spawn → ws-url chain.

### Dependency

- New optional deps behind the `auto-download` feature:
  `zip` (extraction) and `dirs` (platform-appropriate cache
  root). Both inert when the feature is off.

## [0.3.0] — 2026-05-15

Multi-instance release. Operators can declare N browser sessions
in their YAML, each with its own Chrome process + isolated
`user_data_dir`. Existing single-map YAML keeps working via
back-compat shim.

### Breaking

- **Multi-instance config shape.** `[plugin.config_schema]
  shape: "object"` → `"array"`. Operator YAML's `browser:` key
  now accepts either a single map (0.2.x back-compat, normalised
  to a 1-element vec) or a sequence of maps (declared instances).
  `BrowserConfig` gains `instance: Option<String>` + `allow_agents:
  Vec<String>`. `configured_state` cell stores
  `Option<Vec<BrowserConfig>>` instead of `Option<BrowserConfig>`.
- **Tool dispatch routing.** Every `browser_*` tool gains an
  optional `instance: string` arg. Resolution matrix:
    1. explicit instance → looked up in registry; unknown ⇒
       `ArgumentInvalid`.
    2. implicit + 1 declared → uses it (compat shim).
    3. implicit + 0 declared → legacy per-agent_id fallback
       (Phase 81.17.c.multi-profile).
    4. implicit + N > 1 declared → `ArgumentInvalid("multiple
       instances declared")`.
  Plus a per-instance `allow_agents` gate: empty list = accept
  any agent; non-empty list rejects callers outside the list.
- **Supervisor auto-respawn enabled** (`respawn = false` →
  `true`, `max_attempts = 0` → `5`). A single Chrome crash no
  longer leaves the plugin offline.
- **Sanitiser rename.** `sanitize_agent_id` →
  `sanitize_id`. `sanitize_agent_id` retained as a
  `#[deprecated]` alias; `IdError` is the canonical name for
  the error enum (`ProfileIdError` kept as alias).
- Plugin owns `BrowserConfig` + `BrowserConfigFile` locally in
  `nexo_plugin_browser::config`. `nexo_config::BrowserConfig`
  no longer imported. (Inherited from Phase 93.4.e.)

### Added

- **`instance_registry`** — `OnceLock<Arc<DashMap<String,
  Arc<BrowserPlugin>>>>` mirroring telegram/whatsapp plugins.
  `boot::apply_configure` populates it from operator YAML +
  diffs vs prior state so hot-reload unregisters + shuts down
  removed labels.
- **Manifest auto-discovery sections** (Phase 81.33.b.real
  Stages 1+2+4+5+6):
    - `[plugin.capabilities.broker]` subscribe allowlist for 6
      auto-discovery topics under `plugin.browser.*`.
    - `[plugin.pairing] kind = "form"` with `instance` (required)
      + `initial_url` (optional) fields + bilingual instructions.
    - `[plugin.pairing.adapter]` for Stage 1 broker dispatch.
    - `[plugin.http] mount_prefix = "/browser"` (501 today —
      contract reserved).
    - `[plugin.admin] method_prefix = "nexo/admin/browser/"`.
    - `[plugin.metrics] prometheus = true`.
    - `[plugin.dashboard.layout] workspace_walk subdir =
      "browser"` + `auth_check session_dir_files` led by the
      operator-confirmed `.nexo-paired` sentinel.
- **Admin RPC handlers** (`src/admin.rs`): `list_instances`,
  `shutdown`, `restart`, `mark_paired`, `launch_visible` (stub —
  runtime headless-override is a deferred follow-up).
- **`auto_discovery` handlers** (`src/auto_discovery.rs`):
  pairing/http/admin/metrics broker-dispatch entry points. (The
  broker subscriber loop pumping requests through these is a
  deferred follow-up; manifest declarations let daemon-side
  helpers route correctly today.)
- **Per-instance Prometheus metrics** (`src/metrics.rs`):
  `browser_tool_invocations_total{instance, tool, ok}`,
  `browser_tool_latency_seconds{instance, tool}`,
  `browser_chrome_alive{instance}`,
  `browser_chrome_restarts_total{instance}`,
  `browser_instances_configured`.
- **Manifest declares `[plugin.config_schema]`** (Phase 93.1)
  with the array shape's JSON Schema. SDK `on_configure(...)`
  handler (Phase 93.4.a-sdk) receives operator YAML via
  `plugin.configure` JSON-RPC.

### Tests

- `tests/config_migration.rs` (7) — BrowserPluginShape parsing.
- `tests/dispatch_routing.rs` (8) — 4-case routing matrix +
  allow_agents gate.
- `tests/configure_boot.rs` (8) — boot loop populates registry,
  diff-aware reload, dup-label rejection.
- `tests/manifest_parse.rs` (8) — Stages 1-6 + version + broker
  allowlist.
- `tests/e2e_multi_instance.rs` (3) — JSON-RPC wire-level
  multi-instance dispatch.
- `tests/e2e_multi_instance_isolation.rs` (1, gated
  `CHROMIUM_BIN`) — Chrome cookie isolation between instances.
- Inline: `instance_registry::tests` (7),
  `auto_discovery::tests` (13), `metrics::tests` (5).
- 130/130 nextest green, 2 ignored (both gated on `CHROMIUM_BIN`).

### Backward compatibility

- Operator YAML `browser: { headless: true, ... }` (bare map)
  keeps working — normalised to a 1-element vec with
  `instance: None`, registry stays empty, legacy per-agent_id
  fallback (Phase 81.17.c.multi-profile) handles dispatch.
- `sanitize_agent_id` retained as deprecated alias.
- Env-var fallback (`NEXO_PLUGIN_BROWSER_*` vars) keeps working
  when daemon doesn't deliver `plugin.configure`. Removed in 0.4.0
  once proyecto Phase 93.5 closes the daemon-side typed-fields
  deprecation window.

### Deferred follow-ups

- `browser.auto_discovery.subscriber` — wire the broker
  subscription loop pumping requests through `auto_discovery::*`
  handlers. Plugin currently has no broker handle (only
  PluginAdapter). Daemon-side helpers route to the declared
  topics today; plugin acks land once this follow-up ships.
- `browser.launch_visible.runtime` — make admin
  `launch_visible` actually override `headless = true` on the
  next Chrome boot. Today the verb is a no-op acknowledgement.
- `browser.per-instance-supervisor` — currently one subprocess
  hosts all instances; a single Chrome crash + plugin-process
  panic resets all of them. Per-instance supervisor (one Chrome
  crash isolated from siblings) is future scope.
- `browser.0.4.0.deprecate-legacy-per-agent` — flip
  `NEXO_PLUGIN_BROWSER_LEGACY_PER_AGENT` default from `1` to
  `0` once enough operators have migrated.

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
