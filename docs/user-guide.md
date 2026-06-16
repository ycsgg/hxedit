# hxedit User Guide

This guide holds the long-form user reference for `hxedit`: feature bundles,
CLI flags, configuration, commands, release artifacts, and redistribution notes.
The project overview stays in [README.md](../README.md).

## Feature Overview

- Non-destructive byte editing with three distinct operations:
  - overwrite in place
  - real insert
  - tombstone delete
- Full undo / redo across edits, paste, replace, and inspector writes
- Unified text / hex / typed-value search with forward/backward traversal,
  wrap-around, and visible-hit highlighting
- Large-file search uses SIMD `memmem` on clean chunks; chunks containing
  tombstone or replacement edits fall back only where needed
- Built-in format inspectors for ELF, PE/COFF, Mach-O, PNG, ZIP, SQLite,
  PCAP/PCAPNG, GZIP, GIF, BMP, WAV, TAR, and JPEG
- Inspector fields can use format-specific readable editors, including classic
  PCAP UTC packet timestamps, GZIP/PE Unix timestamps, ZIP DOS modification
  times, TAR octal mode/size/mtime, and GIF frame delays
- Hashing for MD5, SHA1, SHA256, SHA512, and CRC32
- Clipboard copy/paste, export, fill/zero/xor/replace transforms
- Read-only synchronized diff page against another file (`:diff`)
- Process memory editing by PID or process name, with region browsing,
  freeze/thaw, and explicit commit commands
- Optional disassembly browsing, symbol search, Sagitta-backed analysis, and
  inline assemble patching

## Performance

Large-file benchmark results and scenario descriptions are tracked in
[docs/performance-report.md](performance-report.md). The report includes the
public benchmark subset, 1 GiB save/search/diff scenarios, viewport reads, peak
RSS, and the commands used to reproduce the measurements.

## Quick Start

Run from source:

```bash
cargo run -- <file>
```

Example:

```bash
cargo run -- --readonly --offset 0x100 --inspector some.bin
```

If you already built the binary:

```bash
hxedit some.bin
```

## Build

`hxedit` ships in three feature bundles:

| Bundle | Build command | Includes |
|---|---|---|
| `core` | `cargo build --release --no-default-features` | Hex editor, inspector, search, diff, hash, copy/paste, export |
| `default` | `cargo build --release` | `core` + process memory editing, disassembly view, instruction search, symbol panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone-backed inline assemble patching + Sagitta-backed `:ana` analysis |
| `sagitta-analysis` add-on | `cargo build --release --features sagitta-analysis` | `default` + Sagitta-backed `:ana` analysis from crates.io `sagitta-rs` |

Notes:

- `default` is the normal build and includes process memory editing.
- `full` enables the optional `hexpatch-keystone` dependency under the local
  crate alias `keystone-engine` for inline assembly patching inside `:dis`, and
  includes Sagitta analysis.
- `sagitta-analysis` enables optional `sagitta-rs` analysis for x86/x64 ELF/PE
  inputs; analysis runs on current logical bytes and does not participate in
  undo/save/search byte semantics.
- There is no separate `:asm` command.

## CLI Flags

| Flag | Description |
|---|---|
| `--readonly` | Open without write access; automatically falls back to read-only when needed |
| `--offset <n\|0xhex>` | Start at a specific byte offset |
| `--pid <PID>` | Attach to a running process by PID for memory editing |
| `--process <NAME>` | Attach to a running process by name for memory editing |
| `--inspector` | Open with the side panel visible on the inspector page |
| `--bytes-per-line <n>` | Bytes shown per row, default `16` |
| `--page-size <n>` | Page-cache read size, default `16384` |
| `--cache-pages <n>` | Page-cache capacity, default `128` |
| `--profile` | Print diagnostics to stderr on exit |
| `--no-color` | Disable colors; `NO_COLOR` also disables styling |
| `--config <path>` | Load settings from a specific config file (TOML) |

## Configuration

`hxedit` reads an optional TOML config file at startup. Resolution order, first
match wins:

1. `--config <path>`
2. `$HXEDIT_CONFIG`
3. `~/.config/hxedit/config.toml` (platform config dir)

A missing file is fine. An existing file that fails to parse, or has unknown
keys, is an error. CLI flags always override the config file, which overrides
built-in defaults.

```toml
[display]
bytes_per_line   = 16              # bytes per row
data_panel_bytes = 16              # bytes decoded in the data panel
inspector_depth  = 1               # structs at this depth and below start collapsed
export_c_width   = 12              # bytes per line in `:export c`
export_py_width  = 16              # bytes per chunk in `:export py`
export_name      = "selection_bytes"  # default identifier for `:export c`/`py`

[behavior]
readonly    = false
inspector   = false                # open with the inspector side panel visible
color       = "auto"               # "auto" | "never" ("never" == --no-color)
search_wrap = true                 # wrap around to the other end when search reaches a boundary

[performance]
page_size   = 16384                # page-cache read size
cache_pages = 128                  # page-cache capacity
```

## Command Reference

| Command | Description |
|---|---|
| `:w` / `:w <path>` / `:wq` | Save / save as / save and quit |
| `:u [n]` / `:redo [n]` | Undo / redo |
| `:g <offset>` / `:g end` / `:g +n` / `:g -n` | Goto |
| `:s [mode]<delim><pattern><delim>` / `:s! ...` | Unified search; default `/text/` searches UTF-8 bytes, `x/hex/` searches raw hex bytes, `b/255/` searches one byte, and `u32/u64/i32/i64` variants search typed integer bytes. `!` searches backward. `:S` remains only as a deprecated hex-search alias during transition |
| `:p` / `:pi` / `:p?` / `:pi?` | Overwrite / insert paste and previews |
| `:c [fmt] [disp]` | Copy the active selection |
| `:export <path>` / `:export c` / `:export py` | Export logical bytes |
| `:xor <key>` / `:xor! <key>` | XOR active selection to clipboard / XOR in place. `key` can be decimal `0..255` or hex `0x00..0xff` |
| `:fill <pattern> <len>` / `:zero <len>` | Overwrite transforms |
| `:re [--force] [mode]<delim><needle><delim><replacement><delim>` / `:re! ...` | Replace using the same modes as `:s`. `:re` is equal-length and asks for `--force` when more than 65535 matches are found; `:re!` allows length changes. Legacy `hex/ascii <needle> -> <replacement>` remains accepted |
| `:hash md5\|sha1\|sha256\|sha512\|crc32` | Hash |
| `:diff <path>` / `:diff -n <N> <path>` / `:diff refresh\|next\|prev\|off` | Show a synchronized page comparing current logical bytes with another file. Visible pages realign inserted/deleted bytes within `N`; equal right-side bytes are gray, changed bytes are yellow on both sides, and missing bytes render as red `__`. `next` / `prev` scan in large progress-reporting steps, block other input while scanning, and Esc cancels |
| `:insp` / `:insp more` | Open inspector / reveal more paginated entries |
| `:format ...` | Force format |

Memory-related commands in `default`, `full`, or other `memory` builds:

| Command | Description |
|---|---|
| `:mem` / `:mem list\|refresh\|info\|freeze\|thaw\|commit\|commit-all` | Open the process-memory side panel, inspect regions, refresh maps, suspend/resume the target, write the active region's replacement spans back (`commit`), or commit every dirty region in virtual-address order (`commit-all`). The panel has maps, process-list, and info views. All views scroll with the mouse wheel or arrow keys; clicking a row changes only the highlight |
| `:w` / `:q` in memory mode | `:w` without a path is equivalent to `:mem commit`; `:w <path>` is rejected, use `:export <path>`. Uncommitted replacements, undo, and redo are kept per region across switches, so `:q` refuses to quit while any region is dirty and summarizes the total; `:q!` discards |
| `:ms [mode]<delim><pattern><delim> [filter...]` / `:ms! ...` | Search readable process regions by virtual address. Modes include text, `x/hex/`, `b/byte/`, `u32/u64`; filters include `in:rw-`, `in:heap`, `not:path:/usr/lib/*`, and `in:va:start-end`. Repeat the last memory search with `gn` / `gN`, independent from file-search `n` / `p` history |

Disassembly-related commands in `default` / `full` builds:

| Command | Description |
|---|---|
| `:dis [arch]` | Enter read-only disassembly view for recognized ELF / PE / Mach-O executables; direct branch jump rails are shown when the text pane is wide enough |
| `:dis! <arch> <offset>` | Force raw disassembly from a display offset |
| `:dis off` | Leave disassembly view |
| `:si` / `:si!` | Search decoded instruction text |
| `:symbol` / `:symbol!` | Search by symbol name |
| `:sym` / `:sym off` | Open / close the symbol panel |
| `:data` / `:data off` | Open / close the cursor-relative data panel |

Sagitta analysis commands in `sagitta-analysis` builds:

| Command | Description |
|---|---|
| `:ana` / `:ana status` / `:ana off` | Run Sagitta on current logical bytes, show analysis state, or clear the Sagitta snapshot. Ready snapshots replace the symbol panel source and annotate disassembly with function labels, target names, and function body rails. Equal-length edits mark analysis outdated, while layout-changing edits require rerunning `:ana` before symbol jumps |

## Release Bundles

Tagged releases publish an explicit `OS * arch * feature` matrix.

Current release matrix:

- `linux` / `x86_64` / `core`
- `linux` / `x86_64` / `default`
- `linux` / `x86_64` / `full`
- `linux` / `aarch64` / `core`
- `linux` / `aarch64` / `default`
- `linux` / `aarch64` / `full`
- `macos` / `aarch64` / `core`
- `macos` / `aarch64` / `default`
- `macos` / `aarch64` / `full`
- `windows` / `x86_64` / `core`
- `windows` / `x86_64` / `default`
- `windows` / `x86_64` / `full`

## Redistribution Notes

The `hxedit` source code in this repository is dual-licensed under either:

- MIT ([licenses/LICENSE-MIT](../licenses/LICENSE-MIT))
- Apache-2.0 ([licenses/LICENSE-APACHE](../licenses/LICENSE-APACHE))

at your option.

`core` and `default` builds do not enable the optional Keystone-assembler
dependency described below.

`full` builds enable the optional Keystone-assembler dependency for inline
assembly patching and the optional MIT-licensed `sagitta-rs` analysis
dependency. When redistributing `full` source bundles or binaries from this
repository, ship the included third-party notices and Keystone FOSS notice /
license / exception files as well; see
[licenses/THIRD_PARTY_NOTICES.txt](../licenses/THIRD_PARTY_NOTICES.txt).

Builds that enable `sagitta-analysis` directly also include `sagitta-rs`; keep
the Sagitta notice in [licenses/THIRD_PARTY_NOTICES.txt](../licenses/THIRD_PARTY_NOTICES.txt)
with redistributed artifacts.
