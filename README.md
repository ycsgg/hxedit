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
- **Visual selection** — select a range of bytes for delete, copy, or hash operations
- **Undo / Redo** — full multi-step undo and redo with `Ctrl+Z` / `Ctrl+Y` or `:undo` / `:redo`
- **Search** — search by ASCII text or hex bytes, forward and backward, with automatic wrap-around
- **Format inspector** — built-in parsing for ELF, PNG, and ZIP structures with field-level editing
- **Hash computation** — compute MD5, SHA1, SHA256, SHA512, or CRC32 of a selection or the entire file
- **Clipboard integration** — copy selections as hex, binary, numeric, or base64 text; paste from clipboard as hex or base64
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

Search wraps around automatically — forward search continues from the start after EOF, backward search continues from the end after BOF.

### Clipboard

| Command | Description |
|---------|-------------|
| `:p [!] [n]` | Overwrite-paste at cursor; `!` = raw bytes; `n` = byte limit |
| `:p? [!] [n]` | Preview overwrite-paste |
| `:pi [!] [n]` | Insert-paste at cursor |
| `:pi? [!] [n]` | Preview insert-paste |
| `:c [fmt] [disp]` | Copy visual selection |

Copy format options: `bin` (binary text), `b` (byte groups, default), `db` (2-byte), `qb` (4-byte)

Copy display options: `r` (raw, default), `nb` (big-endian numeric), `nl` (little-endian numeric), `b64` (base64)

### Hash

| Command | Description |
|---------|-------------|
| `:hash md5` | Compute MD5 |
| `:hash sha1` | Compute SHA-1 |
| `:hash sha256` | Compute SHA-256 |
| `:hash sha512` | Compute SHA-512 |
| `:hash crc32` | Compute CRC32 |

Hashes the visual selection if active, otherwise the entire file.

### Inspector

| Command | Description |
|---------|-------------|
| `:insp` | Toggle inspector panel |
| `:format` | Reset to auto-detected format |
| `:format elf\|png\|zip` | Force a specific format |

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

- Supports ELF, PNG, and ZIP formats
- Works best on a wide terminal; shows a warning if the terminal is too narrow
- Nested sections (e.g. ELF Program Headers) are collapsed by default; use `Space` / `Enter` on a header to expand
- Editable fields can be modified, but PNG/ZIP edits show warnings since structure consistency is not automatically repaired
- Read-only fields report that they are view-only

## Limitations

- Save is rewrite-only (writes a temporary file and renames)
- Overwrite paste stops at EOF; excess bytes are dropped
- Copy is text-only; raw binary clipboard copy is not yet supported
