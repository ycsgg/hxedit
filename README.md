# hxedit

A terminal hex editor for large files, written in Rust.

[中文文档 / Chinese README](README_CN.md)

`hxedit` focuses on correct byte-level editing semantics first: non-destructive editing, full undo/redo, search, format inspection, and optional executable/disassembly browsing.

## Features

- Non-destructive byte editing with three distinct operations:
  - overwrite in place
  - real insert
  - tombstone delete
- Full undo / redo across edits, paste, replace, and inspector writes
- ASCII and hex search with forward/backward traversal, wrap-around, and visible-hit highlighting
- Built-in format inspectors for ELF, PE/COFF, Mach-O, PNG, ZIP, GZIP, GIF, BMP, WAV, TAR, and JPEG
- Hashing for MD5, SHA1, SHA256, SHA512, and CRC32
- Clipboard copy/paste, export, fill/zero/replace transforms
- Large-file support through paged I/O and cache
- Optional disassembly browsing, symbol search, and inline assemble patching

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
|------|------|------|
| `core` | `cargo build --release --no-default-features` | Hex editor, inspector, search, hash, copy/paste, export |
| `default` | `cargo build --release` | `core` + disassembly view, instruction search, symbol panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone-backed inline assemble patching |

Notes:

- `default` is the normal build.
- `full` vendors `keystone-engine` and enables inline assembly patching inside `:dis`.
- There is no separate `:asm` command.

## CLI Flags

| Flag | Description |
|------|-------------|
| `--readonly` | Open without write access; automatically falls back to read-only when needed |
| `--offset <n\|0xhex>` | Start at a specific byte offset |
| `--inspector` | Open with the side panel visible on the inspector page |
| `--bytes-per-line <n>` | Bytes shown per row, default `16` |
| `--page-size <n>` | Page-cache read size, default `16384` |
| `--cache-pages <n>` | Page-cache capacity, default `128` |
| `--profile` | Print diagnostics to stderr on exit |
| `--no-color` | Disable colors; `NO_COLOR` also disables styling |

## Common Commands

| Command | Description |
|---------|-------------|
| `:w` / `:w <path>` / `:wq` | Save / save as / save and quit |
| `:u [n]` / `:redo [n]` | Undo / redo |
| `:g <offset>` / `:g end` / `:g +n` / `:g -n` | Goto |
| `:s <text>` / `:s! <text>` | ASCII search |
| `:S <hex>` / `:S! <hex>` | Hex search |
| `:p` / `:pi` / `:p?` / `:pi?` | Overwrite / insert paste and previews |
| `:c [fmt] [disp]` | Copy the active selection |
| `:export <path>` / `:export c` / `:export py` | Export logical bytes |
| `:fill <pattern> <len>` / `:zero <len>` | Overwrite transforms |
| `:re ...` / `:re! ...` | Equal-length replace / length-changing replace |
| `:hash md5|sha1|sha256|sha512|crc32` | Hash |
| `:insp` / `:insp more` | Open inspector / reveal more paginated entries |
| `:format ...` | Force format |

Disassembly-related commands in `default` / `full` builds:

| Command | Description |
|---------|-------------|
| `:dis [arch]` | Enter read-only disassembly view for recognized ELF / PE / Mach-O executables |
| `:dis! <arch> <offset>` | Force raw disassembly from a display offset |
| `:dis off` | Leave disassembly view |
| `:si` / `:si!` | Search decoded instruction text |
| `:symbol` / `:symbol!` | Search by symbol name |
| `:sym` / `:sym off` | Open / close the symbol panel |
| `:data` / `:data off` | Open / close the cursor-relative data panel |

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

## Limitations

- Save is currently rewrite-save only
- Overwrite paste truncates at EOF instead of auto-appending
- Clipboard copy is still text-oriented rather than raw binary clipboard output

## License

`hxedit` is distributed under `GPL-2.0-only`.
