# TODO

Feature requests parked while the Rust rewrite is in progress. Each entry
records the date it was requested.

- **Custom commands** (2026-08-26): let the user declare their own palette
  entries, beyond the built-in catalog (`commands.json`) and installed plugin
  actions. Catalog loading is isolated in `src/catalog.rs`, so merging a
  user-provided file later stays a local change.

Done:

- **Single-keystroke trigger** (requested and closed 2026-08-26): herdr
  supports prefix-less direct bindings natively — dropping the `prefix+`
  part of a `keys.command` key is all it takes, no plugin change. Documented
  in the README's Keybinding section, including the kitty-keyboard-protocol
  caveat for `ctrl+shift+p`-style chords.
