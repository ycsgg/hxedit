# hxedit

A terminal hex editor for large files, written in Rust.

hxedit provides non-destructive byte-level editing with full undo/redo support, built-in format inspection, and search across files of any size.

## Quick Start

```bash
cargo run -- <file>
```

```bash
hxedit --readonly --offset 0x100 --inspector some.bin
```

## Features

- **Non-destructive editing** — overwrite bytes, insert new bytes, or mark bytes as deleted; all changes are undoable
- **Visual / inspector selection** — operate on a byte range from visual mode or the selected inspector field
- **Undo / Redo** — full multi-step undo and redo with `Ctrl+Z` / `Ctrl+Y` or `:undo` / `:redo`
- **Search** — search by ASCII text or hex bytes, forward and backward, with automatic wrap-around and visible-hit highlighting in the hex grid
- **Format inspector** — built-in parsing for ELF, PNG, ZIP, GZIP, GIF, BMP, WAV, TAR, and JPEG structures with field-level editing
- **Hash computation** — compute MD5, SHA1, SHA256, SHA512, or CRC32 of a selection or the entire file
- **Clipboard integration** — copy selections as hex, binary, numeric, or base64 text; paste from clipboard as hex or base64
- **Batch transforms** — fill repeated patterns, replace matching byte/text sequences, and export selections as raw bytes or C/Python literals
- **Large file support** — paged I/O with configurable cache for responsive editing of files much larger than memory
- **Read-only mode** — automatically falls back to read-only when the file cannot be opened for writing
- **Adaptive colors** — auto-detects terminal color support (true-color / 256-color / 16-color / no color)

## CLI Flags

| Flag | Description |
|------|-------------|
| `--readonly` | Open without write access; auto-detected if the file lacks write permission |
| `--offset <n\|0xhex>` | Start at a specific byte offset |
| `--inspector` | Open with the inspector panel enabled |
| `--bytes-per-line <n>` | Bytes shown per row (default 16) |
| `--page-size <n>` | Page cache read size (default 16384) |
| `--cache-pages <n>` | Page cache capacity (default 128) |
| `--profile` | Print diagnostics to stderr on exit |
| `--no-color` | Disable color styling; also disabled by `NO_COLOR` env var |

## Modes

| Mode | Description |
|------|-------------|
| NORMAL | Navigate, delete, select, enter commands |
| EDIT | Overwrite bytes nibble-by-nibble |
| INSERT | Insert new bytes nibble-by-nibble |
| VISUAL | Select a byte range for operations |
| COMMAND | Enter `:` commands with live hints |
| INSPECT | Browse parsed format fields |
| INSPEDIT | Edit an inspector field inline |

## Keybindings

### Navigation

- `h` `j` `k` `l` / arrow keys — move cursor
- `PageUp` `PageDown` — scroll by page
- `Home` `End` — jump to row start / end

### Editing

- `r` — enter overwrite mode
- `i` — enter insert mode
- `x` — delete current byte (or visual selection)
- `0-9 a-f` — enter hex nibbles in edit/insert mode
- `Backspace` — delete in edit/insert mode
- `Ctrl+Z` / `Ctrl+Y` — undo / redo

### Selection & Search

- `v` — toggle visual selection
- `n` / `p` — repeat search forward / backward
- `:` — enter command mode

### Inspector

- `t` / `Tab` — toggle inspector panel
- `j` `k` / `Up` `Down` — move between fields and headers
- `Space` / `Enter` on a header — collapse or expand the section (`▶` collapsed, `▼` expanded)
- `Enter` on a field — start or submit field edit
- `Esc` — leave edit or inspector

## Commands

### File

| Command | Description |
|---------|-------------|
| `:q` | Quit (refuses if unsaved) |
| `:q!` | Force quit |
| `:w` | Save |
| `:w <path>` | Save as |
| `:wq` | Save and quit |
| `:u [n]` | Undo n changes (default 1) |
| `:redo [n]` | Redo n changes (default 1) |

### Navigation

| Command | Description |
|---------|-------------|
| `:g <offset>` | Go to absolute offset (decimal or `0x` hex) |
| `:g end` | Go to last byte |
| `:g +<delta>` | Go forward by delta |
| `:g -<delta>` | Go backward by delta |
| `:s <text>` | Search ASCII downward |
| `:s! <text>` | Search ASCII upward |
| `:S <hex>` | Search hex bytes downward |
| `:S! <hex>` | Search hex bytes upward |

Search wraps around automatically — forward search continues from the start after EOF, backward search continues from the end after BOF. The current search also highlights all visible hits in the hex grid.

Successful `:g` commands report how many display bytes were moved, e.g. `moved +0x1000 → 0x1234`.

### Clipboard

| Command | Description |
|---------|-------------|
| `:p [!] [n]` | Overwrite-paste at cursor; `!` = raw bytes; `n` = byte limit |
| `:p? [!] [n]` | Preview overwrite-paste |
| `:pi [!] [n]` | Insert-paste at cursor |
| `:pi? [!] [n]` | Preview insert-paste |
| `:c [fmt] [disp]` | Copy the active selection |
| `:export <path>` | Export the active selection as raw bytes to a new file |
| `:export c [name]` | Copy the active selection as a C array literal |
| `:export py [name]` | Copy the active selection as a Python bytes literal |

Copy format options: `bin` (binary text), `b` (byte groups, default), `db` (2-byte), `qb` (4-byte)

Copy display options: `r` (raw, default), `nb` (big-endian numeric), `nl` (little-endian numeric), `b64` (base64)

### Transform

| Command | Description |
|---------|-------------|
| `:fill <hex-pattern> <len>` | Overwrite bytes from cursor with a repeated hex pattern |
| `:zero <len>` | Overwrite bytes from cursor with `00` |
| `:re [hex\|ascii] <needle> -> <replacement>` | Replace all non-overlapping equal-length matches in the active selection or entire file |
| `:re! [hex\|ascii] <needle> -> <replacement>` | Replace with length changes allowed (uses real delete/insert) |

### Hash

| Command | Description |
|---------|-------------|
| `:hash md5` | Compute MD5 |
| `:hash sha1` | Compute SHA-1 |
| `:hash sha256` | Compute SHA-256 |
| `:hash sha512` | Compute SHA-512 |
| `:hash crc32` | Compute CRC32 |

Hashes the active selection (visual or selected inspector field) if active, otherwise the entire file.

### Inspector

| Command | Description |
|---------|-------------|
| `:insp` | Toggle inspector panel |
| `:insp more` | Reveal the next batch of paginated entries beyond the current cap |
| `:format` | Reset to auto-detected format |
| `:format elf\|png\|zip\|gzip\|gif\|bmp\|wav\|tar\|jpeg` | Force a specific format |

## Status Bar

- `len` — display length including deleted slots
- `vis` — bytes that will be written on save
- `sel(span)` — selected display-slot span
- `sel(logical)` — selected logical byte count (skipping deleted)
- `[RO]` — read-only indicator
- `[+]` — unsaved changes indicator

## Editing Model

- **Delete** (`x` in normal mode) marks a byte as deleted — it still occupies a display slot but is skipped on save
- **Insert** (`i` mode) adds new bytes, shifting subsequent content right
- **Overwrite** (`r` mode) replaces bytes in-place without changing the layout
- Deleted bytes display as `XX` in hex and `x` in ASCII

## Inspector Notes

- Supports ELF, PE/COFF, Mach-O, PNG, ZIP, GZIP, GIF, BMP, WAV, TAR, and JPEG formats
- Works best on a wide terminal; shows a warning if the terminal is too narrow
- ELF currently covers headers, program/section tables, dynamic tags, notes/GNU properties, symbols, relocations, hash tables, and version metadata
- PE/COFF currently covers DOS header, COFF header, optional header (PE32/PE32+), and section table with data ranges
- Mach-O currently covers Mach header, load commands, segments/sections with data ranges, and Fat (universal) binaries
- GZIP currently covers fixed/optional header fields, compressed payload range, and trailer metadata
- GIF currently covers the logical screen descriptor, global/local color tables, image blocks, extensions, and trailer
- BMP currently covers the bitmap file header, DIB header variants, optional bit masks / palette ranges, and pixel data
- WAV currently covers the RIFF/WAVE header, paginated top-level chunks, `fmt ` metadata, data ranges, and padding bytes
- TAR currently covers USTAR entry headers, paginated entry lists, and file data ranges
- JPEG currently covers segment markers, APP/SOF/SOS metadata, entropy-coded scan data ranges, and EOI
- Nested sections (e.g. ELF Program Headers / Section Header Table children) are collapsed by default; use `Space` / `Enter` on a header to expand
- The currently selected inspector field highlights its byte range in the hex grid
- Editable fields can be modified, but PNG/ZIP/GZIP/TAR/JPEG edits show warnings since structure consistency is not automatically repaired
- Read-only fields report that they are view-only

## Limitations

- Save is rewrite-only (writes a temporary file and renames)
- Overwrite paste stops at EOF; excess bytes are dropped
- Copy is text-only; raw binary clipboard copy is not yet supported

## CI / Release

- The repository pins Rust to `1.94.1` via `rust-toolchain.toml`, and GitHub Actions installs the same toolchain explicitly
- Every push / pull request runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` on Ubuntu / Windows
- Pushing a tag like `v0.1.0` also builds release archives for Linux x86_64, Linux aarch64, macOS arm64, and Windows x86_64, then publishes a GitHub Release with `SHA256SUMS.txt`
- Intel macOS release artifacts are no longer produced; GitHub-hosted CI only relies on Ubuntu / Windows for regular verification
