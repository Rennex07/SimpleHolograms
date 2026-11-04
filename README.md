# SimpleHolograms

Simple persistent text holograms for Pumpkin. This is a port of [FancyHolograms](https://github.com/FancyMcPlugins/FancyHolograms); see `LICENSE` and `NOTICE` for attribution.

## Installation

Built for Pumpkin **0.2.1+26.3-26.51**. Put `pumpkin_holograms.wasm` in the server's `plugins` directory and restart.

The plugin requires `fs.read.data` and `fs.write.data` permissions for its configuration and saved holograms. On a headless server, approve them in `pumpkin.toml`:

```toml
[plugins.overrides.pumpkinhologram]
allowed_permissions = ["fs.read.data", "fs.write.data"]
```

PumpkinPIE is optional at runtime and provides server-wide placeholders when installed.

## Usage

- `/hologram create <name> <text>` creates a text hologram.
- `/hologram edit <name> addline|setline|removeline|insertbefore|insertafter ...` edits lines. Tab completion suggests existing names, permitted edit operations, and line numbers (including append positions and `insertafter 0`).
- `/hologram list [page]`, `info`, `move`, `movehere`, `delete` and `reload` manage holograms.

Holograms are shared server display entities, not per-player packets. Server-wide PumpkinPIE placeholders are refreshed once per second, including when PumpkinPIE loads after this plugin. Player-specific placeholders such as `%player_name%` cannot be resolved for each viewer on a shared entity and remain literal. Per-player visibility and view permissions are not supported. Item, block and interactive holograms, PlaceholderAPI/MiniPlaceholders compatibility and display-property editing commands are not ported.

## Build

Use Rust 1.96 or later. Dependencies are fetched from crates.io and the published PumpkinPIE `v0.2.5` Git tag.

```sh
rustup target add wasm32-wasip2
cargo fetch --locked
python tools/check_wit.py
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline
cargo build --locked --offline --release --target wasm32-wasip2
```
