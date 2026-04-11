# hxedit

`hxedit` is a Rust TUI hex editor for large files.

## Features

- Three-column layout: offset, raw hex, ASCII
- Hexyl-inspired byte coloring
- Normal mode, hex edit mode, and command mode
- Patch-in-place save for overwrite-only sessions
- Rewrite save for sessions that contain deleted bytes
- Search by ASCII or hex pattern
- Navigation with arrow keys and `hjkl`
- Optional profiling summary with `--profile`

## Keybindings

- `h` `j` `k` `l` or arrow keys: move cursor
- `PageUp` `PageDown`: scroll by page
- `Home` `End`: jump to row start/end
- `n` `p`: jump to next / previous match for the last search
- `i` or `r`: enter hex edit mode
- `x`: mark current byte as deleted
- `:`: enter command mode
- `Esc`: leave edit or command mode

## Commands

- `:q` `:quit`: quit, refuses if there are unsaved changes
- `:q!` `:quit!`: force quit
- `:w` `:write`: save current file
- `:w <path>` `:write <path>`: save as and switch buffer to the new path
- `:wq`: save and quit
- `:g <offset>` `:goto <offset>`: jump to decimal or `0x` offset
- `:s <text>`: search ASCII text
- `:S <hex>`: search hex bytes, for example `:S 7f 45 4c 46`

## Profiling

Use `hxedit --profile <file>` to print startup, first-frame, slow-frame, and cache diagnostics to `stderr` when the editor exits.

## Notes

- Deleted bytes are shown as `XX` in the hex column and `x` in the ASCII column.
- Deletion is logical during editing. The file is compacted only when saved.
- Hex editing is currently supported in the hex column only.
