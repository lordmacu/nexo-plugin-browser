# Changelog

All notable changes to `nexo-plugin-browser` are documented here.
The project adheres to [Semantic Versioning](https://semver.org).

## [Unreleased]

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
