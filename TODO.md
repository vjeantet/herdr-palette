# TODO

Feature requests parked while the Rust rewrite is in progress. Each entry
records the date it was requested.

- **Custom commands** (2026-08-26): let the user declare their own palette
  entries, beyond the built-in catalog (`commands.json`) and installed plugin
  actions. Catalog loading is isolated in `src/catalog.rs`, so merging a
  user-provided file later stays a local change.
- **Single-keystroke trigger** (2026-08-26): replace the two-step trigger
  (prefix, then key — currently `prefix+space`) with a single shortcut. This
  depends on what herdr's `config.toml` accepts as a prefix-less binding, not
  on the plugin itself.
