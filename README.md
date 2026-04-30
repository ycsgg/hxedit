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

## Build Profiles

| Profile | Build command | Includes |
|------|------|------|
| `simple` | `cargo build --release --no-default-features` | Core hex editor, inspector, search, hash, copy/paste, export |
| `default` | `cargo build --release` | `simple` + disassembly view / instruction search / symbol side panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + reserved `asm` feature hook for future assembler backend integration |

`full` is wired today so future assembler support can land without changing the build flavor name; the current tree still does **not** expose a `:asm` command yet.

## Features

- **Non-destructive editing** — overwrite bytes, insert new bytes, or mark bytes as deleted; all changes are undoable
- **Visual / inspector selection** — operate on a byte range from visual mode or the selected inspector field
- **Undo / Redo** — full multi-step undo and redo with `Ctrl+Z` / `Ctrl+Y` or `:undo` / `:redo`
- **Search** — search by ASCII text or hex bytes, forward and backward, with automatic wrap-around and visible-hit highlighting in the hex grid
- **Format inspector** — built-in parsing for ELF, PNG, ZIP, GZIP, GIF, BMP, WAV, TAR, and JPEG structures with field-level editing
- **Hash computation** — compute MD5, SHA1, SHA256, SHA512, or CRC32 of a selection or the entire file
- **Clipboard integration** — copy selections as hex, binary, numeric, or base64 text; paste from clipboard as hex or base64
- **Batch transforms** — fill repeated patterns, replace matching byte/text sequences, and export selections as raw bytes or C/Python literals
- **Optional executable browsing** — default builds include `:dis`, `:si`, and `:sym`; simple builds omit the disassembly / symbol stack entirely
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

### Side Panel

- `t` / `Tab` — toggle the current side panel; after `:sym` or `:data`, reopening restores that page rather than resetting to inspector
- `j` `k` / `Up` `Down` — move between inspector fields/headers or symbol rows
- `PageUp` / `PageDown` / mouse wheel — scroll the focused symbol/data panel or main view
- `Space` / `Enter` on an inspector header — collapse or expand the section (`▶` collapsed, `▼` expanded)
- `Enter` on a field — start or submit field edit; `Enter` on a symbol jumps to its file offset
- Mouse click on a symbol row — select and jump to that symbol's mapped file offset; the bottom detail area shows `symbol / meta / offset / file` and can be mouse-wheel scrolled for very long names
- Mouse click on a data row — select the bytes that row decodes and sync the hex grid selection
- `Esc` — leave edit or side-panel focus

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
| `:si <text>` | Search decoded instruction text downward in disassembly view (`default` / `full`) |
| `:si! <text>` | Search decoded instruction text upward in disassembly view (`default` / `full`) |

Search wraps around automatically — forward search continues from the start after EOF, backward search continues from the end after BOF. The current search also highlights all visible hits in the hex grid.

In disassembly view, byte search results now jump to the containing instruction row, and `:si` / `:si!` search decoded instruction text directly.

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
| `:dis [arch]` | Enter the current read-only disassembly view for recognized ELF / PE / Mach-O executables (`default` / `full`) |
| `:dis! <arch> <offset>` | Force raw disassembly from a display offset even without executable-container detect (`default` / `full`) |
| `:dis off` | Return from disassembly view to the normal hex/ascii view (`default` / `full`) |
| `:sym` | Show executable symbols/import targets in the side panel (`default` / `full`) |
| `:sym off` | Close the symbol page and restore the inspector when available (`default` / `full`) |
| `:data` | Show cursor-relative primitive data decoding in the side panel |
| `:data off` | Close the data page |

Hashes the active selection (visual or selected inspector field) if active, otherwise the entire file.

### Inspector

| Command | Description |
|---------|-------------|
| `:insp` | Toggle inspector panel |
| `:insp more` | Reveal the next batch of paginated entries beyond the current cap |
| `:format` | Reset to auto-detected format |
| `:format elf\|png\|zip\|gzip\|gif\|bmp\|wav\|tar\|jpeg` | Force a specific format |

## Disassembly (Current Stage, `default` / `full` builds)

- `:dis` now delivers a real read-only disassembly pane: executable container detect, arch resolution, backend resolve, and decoded instruction rows in the left main view
- `:dis! arch offset` can force a raw disassembly view on arbitrary bytes; current forced mode assumes little-endian decoding for the chosen arch
- 默认 backend 已抽象为 registry + `CapstoneBackend`，并开启 `capstone/full`；当前实际 decode 支持 `x86` / `x86_64` / `aarch64`
- 左侧 gutter 现在显示 `段名:offset`，非 executable sections / spans 也会以原始字节行显示，右侧用 `.db ...` 占位
- `j` / `k`、PageUp / PageDown、以及主视图滚轮滚动在 `:dis` 下已改为按 instruction row 前后移动，不再沿用 hex row 步长
- 鼠标点击 / 拖拽在 `:dis` 下已按 disassembly rows 命中，不再复用 hex grid 的 offset 换算
- disassembly viewport 现在带有 row cache/checkpoints；重复滚动、搜索定位与重绘不再每次从当前 span 起点重新解码
- `:sym` opens a symbol side panel with a compact `Address / Name` list plus a fixed, scrollable detail area ordered as `symbol`, `meta`, `offset`, `file`; keyboard navigation, PageUp/PageDown, mouse wheel, Enter, and row clicks navigate to mapped file offsets
- disassembly rows 现在会显示基于 `object` baseline symbols 的 `<symbol> @virtual_address` 行尾标签；当操作数字面量精确命中已知 symbol address 时，也会做最小 symbol 名替换
- 对 `x86` / `x86_64` / `aarch64` 的 direct call/jump，当前会额外保留结构化 target metadata；当目标命中已知 symbol 时，行尾会追加轻量 `→` target hint，避免只靠原始立即数阅读
- ELF 下的 direct call target 现在还会补最小 PLT/import 名映射：即使目标地址本身没有 exact symbol，只要能从动态重定位顺序推出对应 PLT slot，也会显示成导入名（例如 `puts`）
- symbol display name 现在会额外清理常见平台修饰，例如 ELF 的 `@@GLIBC_*` / `@plt`、Mach-O/C 的前导 `_`、以及 PE import / stdcall 装饰，降低行尾标签和操作数替换噪音
- symbolized operands、行尾 `<symbol>` 标签与 direct-target symbol hint 现在共用更醒目的 symbol accent color，避免在反汇编文本里和普通 operand 混在一起
- 进入 `:dis` 时状态栏会附带当前已收集的 symbol / import 计数（若存在）
- 当前 dis 主视图仍保持更宽的 instruction text 区域和较窄的 bytes 列，便于 review 指令文本可读性
- `hxedit` 的目标仍是 byte-level editor + executable browsing，不会把 `:dis` 扩成重度 binary analysis 工具；CFG、function graph、decompiler、自动函数恢复都不在近期目标内
- 后续会优先补“方便查看”的轻量能力，例如 direct call/jump target 提示、symbol/import 名字映射、以及 PLT / GOT 一类可浏览元数据，而不是引入完整分析框架
- Disassembly view remains overwrite-only for layout-changing edits such as insert/delete/paste-insert
- 更深入的 import thunk / PLT / GOT / symbol target 解析，以及更细粒度的 patch-triggered cache invalidation 仍在后续阶段

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
- Deeply nested field names / long values now use hanging-wrap continuation, so wrapped text keeps its indent / value column instead of jumping back to the far left
- The currently selected inspector field highlights its byte range in the hex grid
- Editable fields can be modified, but PNG/ZIP/GZIP/TAR/JPEG edits show warnings since structure consistency is not automatically repaired
- Read-only fields report that they are view-only

## Limitations

- Save is rewrite-only (writes a temporary file and renames)
- Overwrite paste stops at EOF; excess bytes are dropped
- Copy is text-only; raw binary clipboard copy is not yet supported

## CI / Release

- The repository pins Rust to `1.94.1` via `rust-toolchain.toml`, and GitHub Actions installs the same toolchain explicitly
- Every push / pull request runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` across the supported `simple` / `default` / `full` feature combinations on Ubuntu / Windows
- Pushing a tag like `v0.1.0` also builds release archives for Linux x86_64, Linux aarch64, macOS arm64, and Windows x86_64, then publishes a GitHub Release with `SHA256SUMS.txt`
- Intel macOS release artifacts are no longer produced; GitHub-hosted CI only relies on Ubuntu / Windows for regular verification
