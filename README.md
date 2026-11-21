# RustedPaint

RustedPaint is a clean-room, native Rust raster editor for Linux. Its initial
goal is a dependable layer-based document core: create and modify a document,
render its visible layers, undo a change, and export a flat image.

## Status

The project currently provides a small Rust document model, layered compositing,
PNG and layered-PDN import, PNG export, undo/redo, and an initial native desktop
editor. The document core remains independent of the optional desktop UI.

```bash
cargo test
cargo run
```

Inspect a flat PNG or supported layered PDN, or render either one to a new PNG:

```bash
cargo run -- inspect image.pdn
cargo run -- render image.pdn composite.png
```

Run the initial native desktop shell on X11 with:

```bash
cargo run --features desktop -- edit
```

The current desktop feature intentionally enables X11 only, avoiding a hard
Wayland runtime dependency while the first UI is under development.

On NixOS, enter the included development shell before launching it:

```bash
nix-shell
cargo run --features desktop -- edit
```

The desktop editor currently includes live Pencil and Eraser brushes,
contiguous Bucket with RGBA tolerance, rectangular selection, staged Move, Copy/Cut/Paste, layer
visibility/opacity/blend mode/reordering, new canvas creation, and PNG/PDN
opening. Pencil and Eraser have a 1–64 pixel square brush; Bucket acts on a
click or completed drag. Move is committed with **Finish Move** or cancelled
with **Cancel Move**.

Keyboard controls: `S` Select, `P` Pencil, `E` Eraser, `F` Bucket, `M` Move,
`Ctrl+Z` Undo, `Ctrl+Y` Redo, `Ctrl+A` Select All, `Ctrl+D` Deselect,
`Ctrl+C` Copy, `Ctrl+X` Cut, `Ctrl+V` Paste, `Enter` finish a live edit,
`Esc` cancel it, and `0` resets zoom.

## Near-term milestones

1. Selection combine modes and richer transform operations.
2. Antialiased selection masks and richer transform operations.
3. Per-mode PDN conformance fixtures and PNG metadata/profile handling.
4. Canvas panning, scrolling, and native file dialogs.

See [docs/clean-room.md](docs/clean-room.md) and
[docs/architecture.md](docs/architecture.md).
