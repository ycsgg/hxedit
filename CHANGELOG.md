# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/ycsgg/hxedit/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/ycsgg/hxedit/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ycsgg/hxedit/releases/tag/v0.1.0
