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
| `default` | `cargo build --release` | `core` + process memory editing, disassembly view, instruction search, symbol panel, Rhai scripts |
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
| `--run <path>` | Run a TOML macro file headlessly and exit; may be repeated |
| `--script <path>` | Run a Rhai script file headlessly and exit; may be repeated in `scripting` builds |
| `--command <cmd>` | Run an exec-compatible command headlessly and exit; may be repeated |
| `--select display:<start>:<len>` / `--select logical:<start>:<len>` | Initial headless selection for `--run` / `--script` / `--command` |
| `--bytes-per-line <n>` | Bytes shown per row, default `16` |
| `--page-size <n>` | Page-cache read size, default `16384` |
| `--cache-pages <n>` | Page-cache capacity, default `128` |
| `--profile` | Print diagnostics to stderr on exit |
| `--no-color` | Disable colors; `NO_COLOR` also disables styling |
| `--config <path>` | Load settings from a specific config file (TOML) |

When `--run`, `--script`, or `--command` is present, `hxedit` opens a file
target, runs the requested automation, prints human-readable summaries, and
exits without creating the TUI. All `--run` files execute first, then all
`--script` files, then all `--command` strings; each group preserves its own
order. Edits are written to disk only if the macro, script, or command list
includes `save`, `hx_save()`, `w`, or `wq`; UI-only commands such as `:diff`,
`:insp`, `:copy`, and clipboard paste are rejected.
For headless `--command`, `hash`, binary `export`, and `replace` use `--select`
when provided and otherwise apply to the whole file; `xor!` requires an explicit
selection.

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
| `:source <path>` | Run a TOML macro file. The macro uses explicit execution-layer steps, can inherit the current Visual / inspector selection, and defaults to grouped undo |
| `:script <path>` | Run a Rhai script file. The script uses the `hx_` host API, inherits the current Visual / inspector selection, and is undoable as one command unless it saves |
| `:diff <path>` / `:diff -n <N> <path>` / `:diff refresh\|next\|prev\|off` | Show a synchronized page comparing current logical bytes with another file. Visible pages realign inserted/deleted bytes within `N`; equal right-side bytes are gray, changed bytes are yellow on both sides, and missing bytes render as red `__`. `next` / `prev` scan in large progress-reporting steps, block other input while scanning, and Esc cancels |
| `:insp` / `:insp more` | Open inspector / reveal more paginated entries |
| `:format ...` | Force format |

## Macro Files

`:source <path>` executes a TOML macro file through the same execution layer as
manual edits. The first implementation is intentionally declarative: it does
not record raw keys and does not run `:diff`, `:insp`, `:sym`, or other UI-only
commands.

The same macro files can run headlessly:

```bash
hxedit sample.bin --run patch.hxmacro
hxedit sample.bin --select display:0x100:16 --run selection_patch.hxmacro
```

Top-level fields:

| Field | Values | Description |
|---|---|---|
| `version` | `1` | Required file format version |
| `selection` | `inherit`, `clear`, `require` | Startup selection policy. TUI `inherit` uses the current Visual / inspector selection; headless `inherit` starts empty unless `--select` is provided |
| `undo` | `group`, `per-step` | Whether edits become one undo step or one step per editing command |
| `on_error` | `stop`, `rollback` | `stop` keeps successful edits undoable; `rollback` reverts successful edits and rejects `save` / `export-binary` steps |

```toml
version = 1
selection = "inherit" # inherit | clear | require
undo = "group"        # group | per-step
on_error = "stop"     # stop | rollback

[[steps]]
cmd = "select"
space = "display"
start = "0x100"
len = 16

[[steps]]
cmd = "xor"
scope = "selection"
key = "0xaa"
in_place = true
```

Common step forms:

| Step | Required fields | Effect |
|---|---|---|
| `goto` | `offset` | Move the execution cursor. Offsets accept decimal, `0x` hex, `cursor`, `cursor+N`, `cursor-N`, or `end` |
| `select` | `space`, `start`, `len` | Set an explicit selection in `display` or `logical` space |
| `clear-selection` | none | Clear the execution selection |
| `read` | `scope`; optional `id` | Read logical bytes from a selection or explicit range. `scope = "all"` is rejected to avoid accidental full-file materialization |
| `hash` | `algorithm`, `scope`; optional `id` | Hash bytes with `md5`, `sha1`, `sha256`, `sha512`, or `crc32` |
| `search` | `pattern`; optional `mode`, `direction`, `select`, `id` | Search from the current cursor. `select = "match"` selects the hit |
| `overwrite` | `offset`, `bytes` | Replacement-only overwrite; does not shift later display offsets |
| `insert` | `offset`, `bytes` | Real insert; shifts later display offsets and clears unstable selections |
| `delete` | `scope`; optional `kind` | Delete a range. Default `kind = "tombstone"` preserves display slots; `kind = "real"` shifts offsets |
| `fill` | `offset`, `pattern`, `len` | Replacement-only repeated pattern overwrite |
| `xor` | `scope`, `key`, `in_place = true` | XOR selected/logical bytes in place |
| `replace` | `scope`, `needle`, `replacement`; optional `mode`, `allow_resize`, `force` | Replace matches; length-changing replacement requires `allow_resize = true` |
| `export-binary` | `scope`, `path` | Export bytes to a file. Relative paths are resolved from the macro file directory |
| `save` | optional `path` | Save in place or save as. A successful save clears undo history before later edits |

Ranges are explicit: `scope = "selection"`, `scope = "all"`, or an inline
range like `scope = { space = "display", start = "0x100", len = 16 }`. Bytes
fields use hex streams by default, such as `"de ad be ef"`; `search` and
`replace` also support `mode = "text"` and `mode = "byte"`.

Result-producing steps can optionally bind an `id`. `read` stores the selected
logical bytes, `hash` stores the digest bytes plus compact lowercase hex text,
and `search` stores the matched pattern bytes when a match is found. Later byte
fields can reference those values:

```toml
[[steps]]
cmd = "hash"
id = "payload_sha256"
algorithm = "sha256"
scope = { space = "display", start = "0x100", len = 0x40 }

[[steps]]
cmd = "insert"
offset = "0x200"
bytes = { from = "payload_sha256", format = "bytes" }

[[steps]]
cmd = "insert"
offset = "0x220"
bytes = { from = "payload_sha256", format = "hex-text" }
```

The `format` field defaults to `bytes`; `hex-text` writes ASCII hex. Variable
references are accepted in byte-valued fields such as `bytes`, `pattern`,
`needle`, and `replacement`.

Example: search for a marker, select the match, compute a CRC32, write the CRC
as ASCII hex after the marker, then save:

```toml
version = 1
selection = "clear"

[[steps]]
cmd = "search"
id = "marker"
pattern = "de ad be ef"
select = "match"

[[steps]]
cmd = "hash"
id = "marker_crc32"
algorithm = "crc32"
scope = "selection"

[[steps]]
cmd = "overwrite"
offset = "cursor+4"
bytes = { from = "marker_crc32", format = "hex-text" }

[[steps]]
cmd = "save"
```

In the TUI, the macro result updates the cursor and selection. In headless
mode, the process exits after the requested macro/script/command list finishes.
No edit is written to disk unless a macro step or later command saves.

Practical macro examples live in [examples/](../examples/):

| File | Use case |
|---|---|
| `firmware_header_crc.hxmacro` | Repair a fixed firmware header CRC and reserved bytes |
| `extract_selected_record.hxmacro` | Export, hash, XOR-decode, and audit an inherited selection |
| `sanitize_log_copy.hxmacro` | Produce a sanitized log copy via replacement and export |
| `strip_debug_marker.hxmacro` | Remove a trailing debug marker and save a trimmed copy |

## Rhai Scripts

`:script <path>` runs a Rhai script in the TUI, and `--script <path>` runs the
same script headlessly. Both paths use the same execution layer. TUI scripts
inherit the current Visual / inspector selection and are pushed as one undo
step unless the script calls `hx_save()`. Rhai scripting is included in
`default` and `full` builds; `core` / `--no-default-features` builds reject
`:script` and `--script` with a feature error.

The script API is intentionally constrained to execution-layer operations and
prefixed with `hx_`:

| Function | Returns | Description |
|---|---|---|
| `hx_hex(text)` | bytes blob | Parse a hex stream such as `"de ad be ef"` |
| `hx_ascii(text)` | bytes blob | Convert text to raw bytes |
| `hx_cursor()` | integer | Current display cursor |
| `hx_len_display()` | integer | Current display length |
| `hx_len_logical()` | integer | Current logical byte length |
| `hx_goto(offset)` | none | Move to an absolute display offset |
| `hx_goto_end()` | none | Move to the final display offset, or `0` for an empty document |
| `hx_select_display(start, len)` | none | Set a display-space selection |
| `hx_select_logical(start, len)` | none | Set a logical-space selection |
| `hx_clear_selection()` | none | Clear the current selection |
| `hx_has_selection()` | bool | Whether a selection is active |
| `hx_selection_start()` | integer | Current selection start; errors without a selection |
| `hx_selection_len()` | integer | Current selection length; errors without a selection |
| `hx_selection_space()` | string | Current selection space: `"display"` or `"logical"` |
| `hx_read_display(start, len)` | bytes blob | Read logical bytes covered by a display range |
| `hx_read_logical(start, len)` | bytes blob | Read logical bytes covered by a logical range |
| `hx_read_selection()` | bytes blob | Read logical bytes from the current selection |
| `hx_search(bytes)` | integer | Compatibility alias for `hx_search_forward(bytes)` |
| `hx_search_forward(bytes)` / `hx_search_backward(bytes)` | integer | Search from the current cursor; returns the display offset or `-1` |
| `hx_search_forward_select(bytes)` / `hx_search_backward_select(bytes)` | integer | Search and select the matched display range |
| `hx_hash_hex(algorithm)` | string | Hash the current selection, or the whole file when no selection exists |
| `hx_hash_display_hex(start, len, algorithm)` | string | Hash a display range |
| `hx_hash_logical_hex(start, len, algorithm)` | string | Hash a logical range |
| `hx_hash_selection_hex(algorithm)` | string | Hash the current selection; errors without a selection |
| `hx_hash_all_hex(algorithm)` | string | Hash all current logical bytes |
| `hx_overwrite(offset, bytes)` | none | Replacement-only overwrite at a display offset |
| `hx_insert(offset, bytes)` | none | Real insert at a display offset |
| `hx_fill(offset, pattern, len)` | none | Replacement-only repeated pattern overwrite |
| `hx_delete_display(start, len)` / `hx_delete_logical(start, len)` / `hx_delete_selection()` | none | Tombstone delete; display slots remain and logical bytes disappear |
| `hx_delete_real_display(start, len)` / `hx_delete_real_logical(start, len)` / `hx_delete_real_selection()` | none | Real delete; following display offsets shift left |
| `hx_xor_display(start, len, key)` / `hx_xor_logical(start, len, key)` / `hx_xor_selection(key)` | none | Replacement-only XOR in place; `key` is `0..255` |
| `hx_replace_all(needle, replacement, allow_resize, force)` | none | Replace all matches in the current logical bytes |
| `hx_replace_display(start, len, needle, replacement, allow_resize, force)` | none | Replace matches in a display range |
| `hx_replace_logical(start, len, needle, replacement, allow_resize, force)` | none | Replace matches in a logical range |
| `hx_replace_selection(needle, replacement, allow_resize, force)` | none | Replace matches in the current selection |
| `hx_export_display(start, len, path)` / `hx_export_logical(start, len, path)` / `hx_export_selection(path)` | none | Export logical bytes to a binary file |
| `hx_save()` | none | Save the current document |
| `hx_save_as(path)` | none | Save to another path |

Relative paths passed to `hx_export_*` and `hx_save_as()` are resolved from the
script file directory. `allow_resize = false` keeps replace operations
replacement-only; `allow_resize = true` permits real delete / insert behavior
and clears the selection.

Default script budgets are `2,000,000` Rhai operations, `100,000` exec calls,
`512 MiB` total bytes returned to the script by reads, `64 MiB` per read, and
`64 MiB` per bytes blob. Hash/search/export style operations still use the
streaming document paths and do not materialize the whole file in the VM unless
the script explicitly calls `hx_read_*`.

Example script:

```rhai
let marker = hx_hex("de ad be ef");

hx_goto(0);
let hit = hx_search_forward_select(marker);
if hit < 0 {
    throw "marker not found";
}

let digest = hx_hash_selection_hex("crc32");
hx_overwrite(hit + 4, hx_ascii(digest));
hx_save();
```

Run it headlessly:

```bash
hxedit sample.bin --script examples/simple_hash_patch.hxscript
```

Or from the TUI command line:

```text
:script examples/simple_hash_patch.hxscript
```

Headless command lists can mix scripts with ordinary exec-compatible commands:

```bash
hxedit sample.bin --command "script patch.hxscript" --command w
```

Use macros when the workflow is a fixed list of declared steps. Use scripts
when you need branching, loops, or when later offsets/bytes depend on earlier
search, read, or hash results.

Practical script examples live in [examples/](../examples/):

| File | Use case |
|---|---|
| `simple_hash_patch.hxscript` | Patch a marker-adjacent CRC32 |
| `extract_payload_between_markers.hxscript` | Export bytes between markers and stamp a SHA-256 |
| `decode_selected_xor.hxscript` | Decode an inherited XOR selection and save audit artifacts |
| `sanitize_log_copy.hxscript` | Sanitize common log tokens and normalize line endings |
| `trim_debug_trailer.hxscript` | Remove debug/volatile markers in a saved copy |

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

Default/full builds include the `scripting` feature and the `MIT OR Apache-2.0`
licensed Rhai dependency; keep the Rhai notice in
[licenses/THIRD_PARTY_NOTICES.txt](../licenses/THIRD_PARTY_NOTICES.txt) with
redistributed artifacts.
