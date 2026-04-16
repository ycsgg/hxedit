# hxedit

`hxedit` is a terminal hex editor for large files, written in Rust.

It is built around a non-destructive byte editing model:

- real inserts / real deletes are tracked by a piece table
- normal deletes are tombstones that keep their display slot
- overwrite edits are replacements on stable `CellId`s

The project is usable today, but it is intentionally narrower than a full-featured binary IDE. The current implementation prioritizes byte-level correctness, undo/save/search stability, and room for large-file performance work.

## Current Status

- Save is currently **rewrite-only**
  - saving the same path writes a temporary file and renames it back
  - there is no true patch-save fast path yet

- Overwrite paste currently **stops at EOF**
  - it does not append the remainder automatically

- Insert paste is supported
  - it inserts bytes at the cursor and shifts following display offsets right

- Clipboard text paste accepts:
  - hex text like `de ad be ef` or `deadbeef`
  - base64 text like `SGVsbG8=`
  - data URLs with `;base64,`

- Built-in format inspector currently supports:
  - ELF
  - PNG
  - ZIP

- Inspector editing is still **byte-oriented, not structure-safe**
  - PNG / ZIP edits now show explicit warnings
  - PNG edits do not repair CRC or chunk consistency
  - ZIP edits do not repair header / descriptor consistency

## Build And Run

```bash
cargo run -- <file>
```

Useful CLI flags:

- `--readonly`
  - open without write access

- `--offset <n|0xhex>`
  - start at a specific byte offset

- `--inspector`
  - open with inspector enabled

- `--bytes-per-line <n>`
  - bytes shown per row, default `16`

- `--page-size <n>`
  - page cache read size, default `16384`

- `--cache-pages <n>`
  - page cache capacity, default `128`

- `--profile`
  - print startup / render / search diagnostics to `stderr` on exit

- `--no-color`
  - disable color styling

Examples:

```bash
cargo run -- tests/fixtures/ascii.bin
cargo run -- --readonly --offset 0x100 --inspector some.bin
```

## Modes

- `NORMAL`
  - move around, delete with tombstones, start selections, enter commands

- `EDIT`
  - overwrite bytes nibble-by-nibble

- `INSERT`
  - insert bytes nibble-by-nibble

- `VISUAL`
  - select a display range for delete / copy

- `COMMAND`
  - enter `:` commands with live hints

- `INSPECT`
  - move through parsed format fields

- `INSPEDIT`
  - edit the selected inspector field inline

## Keybindings

### Main View

- `h` `j` `k` `l` or arrow keys
  - move cursor

- `PageUp` `PageDown`
  - move by one visible page

- `Home` `End`
  - jump to row start / row end

- `v`
  - toggle visual selection

- `i`
  - enter insert hex mode

- `r`
  - enter overwrite hex mode

- `x`
  - tombstone-delete the current byte or the active visual selection

- `n` `p`
  - repeat the last search forward / backward

- `t` or `Tab`
  - toggle the inspector panel

- `:`
  - enter command mode

- `Esc`
  - leave the current sub-mode

### Edit / Insert Modes

- hex digits `0-9 a-f`
  - edit or insert nibbles

- `Backspace`
  - edit-mode / insert-mode backspace

- `Ctrl+Z`
  - undo one edit step

### Command Mode

- `Enter`
  - submit command

- `Esc`
  - cancel command mode

- `Left` `Right` `Home` `End` `Delete` `Backspace`
  - edit the command buffer

### Inspector

- `j` `k` or `Up` `Down`
  - move selected field

- `Left` `Right` `Home` `End` `Delete` `Backspace`
  - edit the current field while in `INSPEDIT`

- `Enter`
  - start editing the selected field, or submit the edit

- `Esc`
  - leave inspector edit, or leave inspector mode

- `:`
  - open command mode from inspector

## Commands

### File / Session

- `:q` `:quit`
  - quit, but refuse if there are unsaved changes

- `:q!` `:quit!`
  - force quit

- `:w` `:write`
  - save current file

- `:w <path>` `:write <path>`
  - save as and switch the buffer to the new path

- `:wq`
  - save and quit

- `:u [steps]` `:undo [steps]`
  - undo one change by default, or more if a positive count is provided

### Navigation / Search

- `:g <offset|end|+delta|-delta>` `:goto <offset|end|+delta|-delta>`
  - jump to an absolute offset, to the final byte with `end`, or relative to the current cursor with `+` / `-`
  - absolute and relative values support decimal or `0x`-prefixed hex

- `:s <text>`
  - search ASCII downward

- `:s! <text>`
  - search ASCII upward

- `:S <hex>`
  - search hex bytes downward, for example `:S 7f 45 4c 46`

- `:S! <hex>`
  - search hex bytes upward

Search does not currently wrap around.

### Paste

- `:p [!] [num]` `:paste [!] [num]`
  - overwrite-paste at the cursor
  - default input is clipboard text parsed as hex first, then base64
  - `!` uses raw clipboard bytes
  - `num` limits how many bytes are used
  - bytes past EOF are currently dropped

- `:p? [!] [num]` `:paste? [!] [num]`
  - preview overwrite-paste without modifying the document

- `:pi [!] [num]` `:paste-insert [!] [num]`
  - insert-paste at the cursor
  - bytes are inserted and subsequent offsets move right

- `:pi? [!] [num]` `:paste-insert? [!] [num]`
  - preview insert-paste without modifying the document

### Copy

- `:c [bin|b|db|qb] [r|nb|nl]`
  - copy the current visual selection to the system clipboard as formatted text

Format options:

- `bin`
  - binary text

- `b`
  - byte groups, default

- `db`
  - 2-byte groups

- `qb`
  - 4-byte groups

Display options:

- `r`
  - raw formatted text, default

- `nb`
  - big-endian numeric output

- `nl`
  - little-endian numeric output

Current limitation:

- copy is text-oriented
- there is no raw binary clipboard copy yet

### Inspector

- `:insp` `:inspector`
  - toggle the inspector panel

- `:format`
  - return to auto-detected inspector format

- `:format elf|png|zip`
  - force a built-in inspector format when the file matches

## Editing Semantics

- Normal delete is a **tombstone delete**
  - the display slot stays visible
  - the byte is skipped on save

- Insert-mode backspace uses **real delete**
  - later display offsets move left immediately

- Overwrite editing and inspector writes use **replacement**
  - the piece layout does not change

- Deleted bytes render as:
  - `XX` in the hex column
  - `x` in the ASCII column

## Inspector Notes

- The inspector is currently rebuilt by full detect + parse + flatten refreshes
- It works best on a wide terminal
- If the terminal is too narrow, inspector focus is rejected and a status warning is shown
- Editable fields are not a promise of structure-safe output
- PNG / ZIP inspector edits show warnings because structure consistency is not repaired automatically

## Limitations

- save is rewrite-only
- overwrite paste truncates at EOF
- no redo yet
- no command history yet
- no wrap-around search yet
- opening an unwritable file does not auto-fallback to readonly
- copy is text-only, not raw-binary

## Profiling

Use:

```bash
hxedit --profile <file>
```

to print startup, render, search, and cache diagnostics to `stderr` when the editor exits.
