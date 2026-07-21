# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.1] - 2026-07-21

### Added

- Session-local bookmarks and comments for offsets, ranges, selections, and inspector fields, with optional names, colors, and notes.
- A bookmark side panel with keyboard and mouse navigation, plus bookmark markers across the gutter, hex grid, and ASCII grid.

### Changed

- Process-memory bookmarks are isolated per region and restored only while the region revision remains current.

### Fixed

- Equal-length `:re!` operations now retain in-place replacement semantics, including in overwrite-only views.
- Resizing replacements now apply the large-match confirmation guard before collecting the full match set.

## [0.5.0] - 2026-06-29

### Added

- Remote file editing for `sftp://...` and `ssh://...` targets is now included in the default build through the Rust `russh` + `russh-sftp` stack.
- Optional `remote-ftp` / `remote-all` feature builds add passive binary FTP support through `suppaftp`.
- Remote save now rewrites through an exclusive remote temporary file, checks the open-time fingerprint before replacing the target, preserves dirty state on failure, and cleans up temporary files on aborted saves.

### Changed

- `ssh://` is now an alias for the SFTP subsystem transport instead of a remote shell command fallback.
- The old OpenSSH command, OpenSSH SFTP wrapper, and libssh2 SFTP backends were removed in favor of a single `russh-sftp` backend.
- Release builds now strip symbols to reduce binary size.
- Remote documentation now describes the supported authentication paths and explicitly calls out unsupported OpenSSH config, ProxyJump, and GSSAPI behavior.

### Fixed

- FTP passive connections now use `suppaftp` instead of maintaining FTP control response and PASV parsing in hxedit.

## [0.4.0] - 2026-06-18

### Added

- Headless macro execution through `--run` and TOML macro files.
- Rhai scripting through `--script` and the `hx_` host API.
- Shared execution layer for TUI commands, headless macros, and scripts.

### Changed

- Reworked README and user-guide documentation around feature bundles, automation, release artifacts, and redistribution notes.
- Replaced the previous disassembly backend stack with pure Rust `iced-x86` and `yaxpeax-arm` decoders.
- Optimized disassembly decode and render hot paths.

### Fixed

- Fixed clippy failures in script-enabled builds.

## [0.3.3] - 2026-06-15

### Added

- Public large-file performance benchmark documentation and reproduction commands.

### Changed

- Bounded the disassembly cache and expanded performance benchmarks.
- Optimized paste overwrite replacements and mixed replacement edits.

### Fixed

- Fixed clippy failures in no-default-feature builds.

## [0.3.2] - 2026-06-14

### Added

- Lightweight SQLite, PCAP, and PCAPNG inspectors
- Deeper ZIP parsing, including central directory, EOCD, ZIP64, and data descriptor awareness
- Readable editable fields for common timestamps and packed values, including classic PCAP packet timestamps, GZIP/PE Unix timestamps, ZIP DOS modification time, TAR octal mode/size/mtime, and GIF frame delay

### Changed

- `:re` now accepts the same mode-delimiter syntax as `:s`, including `/text/`, `x/hex/`, `b/byte/`, and typed integer modes; the legacy `hex/ascii <needle> -> <replacement>` form remains supported
- Clean `:fill`, `:zero`, `:xor!`, and equal-length clean `:re` ranges now use compact replacement range overlays and bulk undo records instead of storing per-byte undo data
- Equal-length `:re` now requires `--force` when more than 65535 matches are found, and `:re --force` scans and applies in batches

### Fixed

- `:diff next` / `:diff prev` now scan large files in cancellable progress-reporting steps instead of blocking the UI until the full scan finishes
- Diff projection rendering is more efficient for visible pages
- Clean and sparse-dirty search paths use chunked `memmem` scanning, avoiding whole-document KMP fallback after small edits

## [0.3.1] - 2026-06-13

### Added

- Sagitta-backed analysis mode behind the `sagitta-analysis` feature, including `:ana` commands, recovered-function symbol entries, direct-target labels, and function rails in disassembly views
- Disassembly jump rails for direct branch and call targets

### Changed

- `default` builds now include process memory editing
- `full` builds now include Sagitta analysis in addition to Keystone-backed inline assembly patching
- Status-line selection length now counts logical bytes without materializing the selected byte range

### Fixed

- Fixed raw-gap disassembly span boundaries
- Fixed clippy failures in no-default-feature builds

## [0.3.0] - 2026-06-04

### Added

- Process memory editing mode with `--pid <PID>` and `--process <NAME>` CLI flags, plus the `:mem` command family for attaching to, inspecting, and editing live process memory
- Memory panel views (region list, process picker, aggregated info report) with region highlighting and mouse interaction
- Memory freeze and thaw commands (`:mem freeze` / `:mem thaw`) to suspend and resume the target process
- Per-region editing state with `:w` / `:q` commit semantics; uncommitted replacements, undo, and redo are kept per region across switches
- Aggregated `:mem info` report across the whole session
- Memory search (`:ms` / `:ms!`) with `gn` / `gN` repeat, independent of file-search history
- Unified search command syntax (`:s` replaces the deprecated `:S` hex-search alias)
- Read-only synchronized diff page (`:diff`) comparing current logical bytes against another file
- Fixed hex column header (`00 01 02 ... 0F`) for quick column lookup while scrolling
- XOR selection command (`:xor` / `:xor!`) to XOR the active selection to clipboard or in place
- Performance benchmarks via `cargo bench`

### Changed

- The repository source code is now dual-licensed under `MIT OR Apache-2.0`; `full` builds continue to use Keystone and now ship explicit third-party / FOSS notice / license / exception files under a dedicated `licenses/` layout in release archives
- `:export`, `:fill`, and `:xor!` now stream output to avoid full materialization in memory
- Clean-document search uses SIMD `memmem` for faster scanning; only chunks containing tombstone or replacement edits fall back to byte-at-a-time
- Refactored app render, events, and commands for cleaner separation

### Fixed

- Edit nibble no-op undo: editing a nibble no longer creates an empty undo entry
- Memory panel row mapping and scrolling now correctly track the cursor position

## [0.2.0] - 2026-05-06

### Added

- Read-only disassembly view for recognized ELF, PE/COFF, and Mach-O executables via `:dis`, plus forced raw decode via `:dis!`
- Instruction-text search (`:si` / `:si!`), symbol search (`:symbol` / `:symbol!`), symbol side panel (`:sym`), and cursor-relative data side panel (`:data`)
- Symbol-aware disassembly rendering with direct-target hints, PLT/import name resolution, and symbol-name cleanup for common platform decorations
- Inline assemble patching in `full` builds through Keystone-backed single-instruction overwrite edits
- Release artifacts are now published as an explicit `OS * arch * feature` matrix across `core`, `default`, and `full` bundles

### Changed

- The side panel is no longer inspector-only; inspector, symbol, and data pages now share one panel model and focus mode
- The repository license is now `GPL-2.0-only`, and release archives include the license file alongside the binary and README
- README was reorganized into separate Chinese and English sections and updated for the disassembly / feature-bundle release model

## [0.1.1] - 2026-04-23

### Added

- **New format inspectors:**
  - BMP (Bitmap) with header, info header, color table, and pixel data
  - GIF with header, global/local color tables, graphic control extensions, and image data blocks
  - JPEG with segment markers, quantization tables, Huffman tables, and scan data
  - GZIP with header fields, optional filename/comment, and trailer checksum
  - TAR with entry headers, file data ranges, and pagination support
  - WAV with RIFF header, format chunk, and data chunk
  - PE/COFF (Windows executables) with DOS stub, PE header, section headers, and optional header
  - Mach-O (macOS/iOS executables) with header, load commands, symbol tables, and section data
- **Enhanced ELF inspector:**
  - Split into modular structure (header, layout, payloads, structures, symbols, versions)
  - Section headers with name resolution and pagination
  - Dynamic segments and program interpreter
  - Symbol tables (`.symtab`, `.dynsym`) with string table resolution
  - Relocation entries (`.rela.*`, `.rel.*`)
  - GNU property notes and GNU hash table
  - Version definitions (`Verdef`) and requirements (`Verneed`)
  - Comprehensive test coverage for all ELF features
- Inspector jump now centers the target row in hex view for better context
- Nested struct wrapping improved for better readability in inspector panel

### Fixed

- TAR format detection relaxed to handle files with stale checksums

## [0.1.0] - 2026-04-18

Initial release.

### Added

- Byte-level editing with three distinct semantics: overwrite (replacement), insert (real insert via piece table), and delete (tombstone that keeps the display slot but is skipped on save)
- Full undo / redo across edits, inserts, deletes, paste, and inspector writes
- Visual selection mode with display-span and logical-byte reporting in the status bar
- Forward / backward search for ASCII text and hex bytes, with automatic wrap-around and visible-hit highlighting in the hex grid
- Built-in format inspector for ELF (including Program Header Table), PNG, and ZIP, with collapsible nested structs and per-field hex-grid highlighting
- `:insp more` to reveal additional PNG / ZIP entries past the default cap
- `:hash md5 | sha1 | sha256 | sha512 | crc32` over a selection or the entire file, streamed in 64 KB chunks; result copied to the clipboard when available
- Clipboard commands: `:c` (hex / binary / numeric / base64 text formats), `:p` / `:pi` overwrite / insert paste with live preview (`:p?` / `:pi?`)
- Transforms: `:fill <pattern> <len>`, `:zero <len>`, `:re` (equal-length replace), `:re!` (real delete + insert)
- `:export` of selections to raw files, C array literals, or Python bytes literals
- `:g` goto with absolute offset, `end`, and relative `+delta` / `-delta` forms, with moved-by status feedback
- Paged file I/O with configurable `--page-size` and `--cache-pages` for files larger than memory
- Automatic read-only fallback when the file cannot be opened for writing
- Adaptive color output (truecolor / 256-color / 16-color / no-color) with `NO_COLOR` environment variable and `--no-color` flag support
- Command history navigation via Up / Down in command mode
- Rust 1.94.1 toolchain pin via `rust-toolchain.toml`
- CI on Ubuntu and Windows (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --all-targets`)
- Release archives for Linux x86_64, Linux aarch64, macOS arm64, and Windows x86_64, published with `SHA256SUMS.txt`

[0.5.1]: https://github.com/ycsgg/hxedit/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/ycsgg/hxedit/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/ycsgg/hxedit/compare/v0.3.3...v0.4.0
[0.3.3]: https://github.com/ycsgg/hxedit/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/ycsgg/hxedit/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/ycsgg/hxedit/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ycsgg/hxedit/compare/v0.2.1...v0.3.0
[0.2.0]: https://github.com/ycsgg/hxedit/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/ycsgg/hxedit/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ycsgg/hxedit/releases/tag/v0.1.0
