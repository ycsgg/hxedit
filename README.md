# hxedit

中文说明在前，English section follows below.

---

## 中文

`hxedit` 是一个面向大文件的终端十六进制编辑器，使用 Rust 编写。

它优先保证 byte 级编辑语义正确，支持非破坏式编辑、完整 undo/redo、格式检查器、搜索，以及可选的可执行文件反汇编浏览。

### 快速开始

```bash
cargo run -- <file>
```

```bash
hxedit --readonly --offset 0x100 --inspector some.bin
```

### 构建档位

| 档位 | 构建命令 | 包含能力 |
|------|------|------|
| `core` | `cargo build --release --no-default-features` | 核心 hex editor、inspector、search、hash、copy/paste、export |
| `default` | `cargo build --release` | `core` + `:dis` 反汇编浏览、指令搜索、symbol side panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone 驱动的反汇编内联 patch |

`full` 会启用 `asm` 特性并 vendor `keystone-engine`。当前没有单独的 `:asm` 命令，汇编 patch 直接发生在 `:dis` 视图内。

### 核心特性

- 非破坏式编辑：overwrite、insert、tombstone delete 全部可撤销
- Visual 选区与 inspector 字段选区
- Undo / Redo
- ASCII / hex 搜索，支持前后向、自动 wrap-around、同屏命中高亮
- 内置格式检查器：ELF、PE/COFF、Mach-O、PNG、ZIP、GZIP、GIF、BMP、WAV、TAR、JPEG
- 哈希：MD5、SHA1、SHA256、SHA512、CRC32
- 剪贴板复制 / 粘贴、导出、批量 fill / zero / replace
- 可选 executable browsing：`default` / `full` 档位提供 `:dis`、`:si`、`:symbol`、`:sym`
- 大文件分页读取与缓存
- 自动只读回退
- 终端颜色能力自动检测

### CLI 参数

| 参数 | 说明 |
|------|------|
| `--readonly` | 只读打开；如果文件不可写也会自动退回只读 |
| `--offset <n\|0xhex>` | 从指定偏移开始 |
| `--inspector` | 启动时显示 side panel 的 inspector 页 |
| `--bytes-per-line <n>` | 每行显示字节数，默认 `16` |
| `--page-size <n>` | 页缓存读取大小，默认 `16384` |
| `--cache-pages <n>` | 页缓存容量，默认 `128` |
| `--profile` | 退出时向 stderr 输出诊断信息 |
| `--no-color` | 禁用颜色；`NO_COLOR` 环境变量同样生效 |

### 模式

| 模式 | 说明 |
|------|------|
| `NORMAL` | 导航、删除、选区、进入命令 |
| `EDIT` | nibble 级 overwrite |
| `INSERT` | nibble 级 insert |
| `VISUAL` | 字节范围选区 |
| `COMMAND` | `:` 命令输入 |
| `PANEL` | 聚焦当前 side panel 页 |
| `INSPEDIT` | inspector 字段内联编辑 |
| `ASMEDIT` | 反汇编单条指令内联编辑 |

### 常用按键

- `h j k l` / 方向键：移动光标
- `PageUp` / `PageDown`：翻页
- `r`：进入 overwrite
- `i`：进入 insert
- `x`：删除当前字节或 visual 选区
- `0-9 a-f`：编辑十六进制 nibble
- `Ctrl+Z` / `Ctrl+Y`：undo / redo
- `v`：切换 visual
- `:`：进入命令模式
- `t` / `Tab`：切换 side panel

### 反汇编相关

`default` / `full` 档位支持：

- `:dis [arch]`：进入已识别 ELF / PE / Mach-O 的只读反汇编视图
- `:dis! <arch> <offset>`：强制从 display offset 开始做 raw disassembly
- `:dis off`：退出反汇编视图
- `:si` / `:si!`：按指令文本搜索
- `:symbol` / `:symbol!`：按 symbol 名搜索
- `:sym` / `:sym off`：显示 / 关闭 symbol panel
- `:data` / `:data off`：显示 / 关闭 cursor-relative data panel

`full` 档位额外支持：

- 在 `:dis` 里按 `Enter` 进入当前指令的单行编辑
- 提交后使用 Keystone 组装并做 overwrite patch
- 当前仍保持 overwrite-only；layout-changing 编辑不会在反汇编视图里开放

### 常用命令

| 命令 | 说明 |
|------|------|
| `:w` / `:w <path>` / `:wq` | 保存 / 另存 / 保存退出 |
| `:u [n]` / `:redo [n]` | undo / redo |
| `:g <offset>` / `:g end` / `:g +n` / `:g -n` | 跳转 |
| `:s <text>` / `:s! <text>` | ASCII 搜索 |
| `:S <hex>` / `:S! <hex>` | hex 搜索 |
| `:p` / `:pi` / `:p?` / `:pi?` | overwrite / insert paste 及预览 |
| `:c [fmt] [disp]` | 复制当前 active selection |
| `:export <path>` / `:export c` / `:export py` | 导出逻辑字节 |
| `:fill <pattern> <len>` / `:zero <len>` | overwrite 批量写入 |
| `:re ...` / `:re! ...` | 等长替换 / 允许长度变化的替换 |
| `:hash md5|sha1|sha256|sha512|crc32` | 哈希 |
| `:insp` / `:insp more` | 打开 inspector / 加载更多分页项 |
| `:format ...` | 强制格式 |

### 编辑模型

- `x` 是 tombstone delete：保留 display slot，save 时跳过
- `i` 是 real insert：后续 display offset 右移
- `r` 是 replacement：只覆盖现有字节显示值，不改布局

### 限制

- 保存当前仍是 rewrite-save
- overwrite paste 到 EOF 会截断，不会自动 append
- 剪贴板 copy 目前仍以文本表示为主，不直接写 raw binary clipboard

### CI / Release

- Rust 固定为 `1.94.1`
- GitHub Actions 在 Ubuntu / Windows 跑 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --all-targets`
- CI 会覆盖 `core` / `default` / `full` 三个 feature 档位
- 打 `v0.2.0` 这类 tag 时，release 产物按 `OS * arch * feature` 矩阵打包并发布
- 当前发布矩阵：
  - `linux` / `x86_64` / `core|default|full`
  - `linux` / `aarch64` / `core|default|full`
  - `macos` / `aarch64` / `core|default|full`
  - `windows` / `x86_64` / `core|default|full`

### 许可证

`hxedit` 以 `GPL-2.0-only` 发布。`full` 档位中 vendor 的 `keystone-engine` 位于同一许可边界内。

---

## English

`hxedit` is a terminal hex editor for large files, written in Rust.

It prioritizes correct byte-level editing semantics, with non-destructive editing, full undo/redo, built-in format inspection, search, and optional executable/disassembly browsing.

### Quick Start

```bash
cargo run -- <file>
```

```bash
hxedit --readonly --offset 0x100 --inspector some.bin
```

### Build Profiles

| Profile | Build command | Includes |
|------|------|------|
| `core` | `cargo build --release --no-default-features` | Core hex editor, inspector, search, hash, copy/paste, export |
| `default` | `cargo build --release` | `core` + disassembly view, instruction search, symbol side panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone-backed inline assemble patching |

`full` enables the `asm` feature and vendors `keystone-engine`. There is still no separate `:asm` command; patching happens directly inside `:dis`.

### Features

- Non-destructive editing: overwrite, insert, and tombstone delete with full undo
- Visual selection and inspector-field selection
- Undo / Redo
- ASCII / hex search with forward/backward traversal, wrap-around, and visible-hit highlighting
- Built-in inspectors for ELF, PE/COFF, Mach-O, PNG, ZIP, GZIP, GIF, BMP, WAV, TAR, and JPEG
- Hashing: MD5, SHA1, SHA256, SHA512, CRC32
- Clipboard copy/paste, export, fill/zero/replace transforms
- Optional executable browsing: `default` / `full` builds provide `:dis`, `:si`, `:symbol`, and `:sym`
- Paged I/O and cache for files much larger than memory
- Automatic read-only fallback
- Adaptive terminal color support

### CLI Flags

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

### Modes

| Mode | Description |
|------|-------------|
| `NORMAL` | Navigate, delete, select, enter commands |
| `EDIT` | Nibble-by-nibble overwrite |
| `INSERT` | Nibble-by-nibble insert |
| `VISUAL` | Byte-range selection |
| `COMMAND` | `:` command entry |
| `PANEL` | Focus the active side-panel page |
| `INSPEDIT` | Inline inspector-field editing |
| `ASMEDIT` | Inline single-instruction disassembly editing |

### Common Keys

- `h j k l` / arrow keys: move cursor
- `PageUp` / `PageDown`: page scroll
- `r`: enter overwrite
- `i`: enter insert
- `x`: delete current byte or visual selection
- `0-9 a-f`: enter hex nibbles
- `Ctrl+Z` / `Ctrl+Y`: undo / redo
- `v`: toggle visual mode
- `:`: command mode
- `t` / `Tab`: toggle side panel

### Disassembly

Available in `default` / `full` builds:

- `:dis [arch]`: enter read-only disassembly view for recognized ELF / PE / Mach-O executables
- `:dis! <arch> <offset>`: force raw disassembly from a display offset
- `:dis off`: leave disassembly view
- `:si` / `:si!`: search decoded instruction text
- `:symbol` / `:symbol!`: search by symbol name
- `:sym` / `:sym off`: open / close the symbol panel
- `:data` / `:data off`: open / close the cursor-relative data panel

Extra in `full` builds:

- Press `Enter` in `:dis` to edit the current instruction inline
- Submission uses Keystone to assemble and overwrite-patch bytes in place
- Disassembly mode remains overwrite-only; layout-changing edits stay blocked there

### Common Commands

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

### Editing Model

- `x` is tombstone delete: the display slot remains, but save skips it
- `i` is real insert: following display offsets shift right
- `r` is replacement: overwrite visible bytes in place without changing layout

### Limitations

- Save is currently rewrite-save only
- Overwrite paste truncates at EOF instead of auto-appending
- Clipboard copy is still text-oriented rather than raw binary clipboard output

### CI / Release

- Rust is pinned to `1.94.1`
- GitHub Actions runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` on Ubuntu and Windows
- CI covers all three feature bundles: `core`, `default`, and `full`
- A tag such as `v0.2.0` publishes release archives using an explicit `OS * arch * feature` matrix
- Current release matrix:
  - `linux` / `x86_64` / `core|default|full`
  - `linux` / `aarch64` / `core|default|full`
  - `macos` / `aarch64` / `core|default|full`
  - `windows` / `x86_64` / `core|default|full`

### License

`hxedit` is distributed under `GPL-2.0-only`. The vendored `keystone-engine` used by the `full` bundle lives under the same release boundary.
