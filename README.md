# nexo-plugin-browser

Out-of-tree subprocess plugin for the [nexo-rs](https://github.com/nexo-rs) daemon.
Ships the 12 `browser_*` tools (Chrome DevTools Protocol via raw CDP) as a
standalone binary loaded by the daemon at boot via discovery + auto-subprocess
fallback (Phase 81.17 + 81.17.b).

> **Skeleton.** Detailed sections (install, ENV reference, sandbox, latency
> budget) ship in step 13 of the extraction. This README is the entry-point
> placeholder.
