# hxedit Automation Examples

These examples are meant to be copied to a work directory and adjusted for the
target file layout. Macro and script relative output paths are resolved from the
automation file directory.

## Macro Files

Run a macro from the TUI:

```text
:source path/to/file.hxmacro
```

Run a macro headlessly:

```bash
hxedit sample.bin --run path/to/file.hxmacro
```

| File | Practical use | Covered commands |
|---|---|---|
| `firmware_header_crc.hxmacro` | Recompute a payload CRC32 into a fixed header field, clear reserved bytes, and duplicate the image magic as a footer marker | `read`, `hash`, `overwrite`, `fill`, `insert`, `save` |
| `extract_selected_record.hxmacro` | Export and hash the current selection, XOR-decode it in place, then save an audited copy | `selection = "require"`, `read`, `hash`, `export-binary`, `xor`, `clear-selection`, `insert`, `save` |
| `sanitize_log_copy.hxmacro` | Normalize a binary log copy without changing the original file in place | `replace`, `export-binary` |
| `strip_debug_marker.hxmacro` | Find and remove a trailing debug marker, then write a trimmed copy | `goto`, backward `search`, `delete kind = "real"`, `save path` |

## Rhai Scripts

Run a script from the TUI:

```text
:script path/to/file.hxscript
```

Run a script headlessly:

```bash
hxedit sample.bin --script path/to/file.hxscript
```

| File | Practical use | Covered API |
|---|---|---|
| `simple_hash_patch.hxscript` | Search a marker, CRC32 it, and write the digest after the marker | search/select/hash/overwrite/save |
| `extract_payload_between_markers.hxscript` | Carve bytes between text markers, export them, and stamp a SHA-256 digest into a sidecar copy | `hx_search_forward`, `hx_select_display`, `hx_export_selection`, `hx_hash_selection_hex`, `hx_save_as` |
| `decode_selected_xor.hxscript` | Decode an inherited selection with XOR, export before/after payloads, and prepend audit hashes | selection helpers, `hx_xor_selection`, `hx_insert`, `hx_save_as` |
| `sanitize_log_copy.hxscript` | Replace common sensitive log tokens and normalize CRLF to LF in a saved copy | `hx_replace_all`, `hx_save_as` |
| `trim_debug_trailer.hxscript` | Strip a trailing debug marker and tombstone-delete a volatile marker in a saved copy | `hx_goto_end`, backward search/select, real delete, tombstone delete |
