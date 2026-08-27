# TODO

Feature requests parked while the Rust rewrite is in progress. Each entry
records the date it was requested.

Done:

- **Custom commands** (requested 2026-08-26, closed 2026-08-27): user-declared
  palette entries, read from `$HERDR_PLUGIN_CONFIG_DIR/config.toml` and shown
  ahead of the built-in catalog as `User: <title>`. Each entry names an `argv`
  array, which runs in a pane of its own through this plugin's `runner`
  entrypoint — never through a shell. Documented in the README's
  [Your own commands](README.md#your-own-commands); the design document's
  out-of-scope list carries the dated amendment.

- **Single-keystroke trigger** (requested and closed 2026-08-26): herdr
  supports prefix-less direct bindings natively — dropping the `prefix+`
  part of a `keys.command` key is all it takes, no plugin change. Documented
  in the README's Keybinding section, including the kitty-keyboard-protocol
  caveat for `ctrl+shift+p`-style chords.
