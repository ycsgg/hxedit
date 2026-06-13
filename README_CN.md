# hxedit

面向大文件的终端十六进制编辑器，使用 Rust 编写。

主 README 在这里：[README.md](README.md)

`hxedit` 优先保证 byte 级编辑语义正确，提供非破坏式编辑、完整 undo/redo、搜索、格式检查器，以及可选的可执行文件反汇编浏览。

## 功能

- 固定的 byte 列头（默认 `00 01 02 ... 0F`），滚动时仍方便按列定位
- 三种明确区分的 byte 编辑操作：
  - 原地 overwrite
  - real insert
  - tombstone delete
- 编辑、粘贴、替换、inspector 写入都支持完整 undo / redo
- 统一文本 / hex / typed-value 搜索，支持前后向、自动 wrap-around、同屏命中高亮；大文件用 SIMD `memmem` 扫描（只有含 tombstone / replacement 编辑的分块才退回逐字节）
- 内置格式检查器：ELF、PE/COFF、Mach-O、PNG、ZIP（central directory、EOCD、ZIP64、data descriptor）、SQLite、PCAP/PCAPNG、GZIP、GIF、BMP、WAV、TAR、JPEG
- Inspector 字段支持按格式定制的可读编辑器，包括 classic PCAP UTC packet timestamp、GZIP/PE Unix timestamp、ZIP DOS 修改时间、TAR octal mode/size/mtime、GIF frame delay
- 哈希：MD5、SHA1、SHA256、SHA512、CRC32
- 剪贴板复制 / 粘贴、导出、fill / zero / xor / replace
- 只读同步滚动 diff 页面，可用 `:diff` 对比另一个文件
- 进程内存编辑：通过 PID 或进程名附加到运行中的进程，浏览和编辑内存区域，冻结/解冻目标进程，并将修改写回
- 分页 I/O 和缓存，适合大文件
- 可选的反汇编浏览、symbol 搜索、Sagitta 后台分析、内联汇编 patch

## 快速开始

从源码运行：

```bash
cargo run -- <file>
```

示例：

```bash
cargo run -- --readonly --offset 0x100 --inspector some.bin
```

如果已经构建好二进制：

```bash
hxedit some.bin
```

## 自行构建

`hxedit` 提供三种 feature bundle：

| 档位 | 构建命令 | 包含内容 |
|------|------|------|
| `core` | `cargo build --release --no-default-features` | Hex editor、inspector、search、diff、hash、copy/paste、export |
| `default` | `cargo build --release` | `core` + 进程内存编辑、反汇编视图、指令搜索、symbol panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone 驱动的内联汇编 patch + Sagitta `:ana` 分析 |
| `sagitta-analysis` 附加项 | `cargo build --release --features sagitta-analysis` | `default` + 基于 crates.io `sagitta-rs` 的 `:ana` 分析 |

说明：

- `default` 是常规构建档位，并包含进程内存编辑。
- `full` 会启用可选的 `hexpatch-keystone` 依赖（在本仓库里仍使用 `keystone-engine` 这个本地依赖别名），开启 `:dis` 内的 inline assembly patch，并包含 Sagitta 分析。
- `sagitta-analysis` 启用可选的 `sagitta-rs` 分析，面向 x86/x64 ELF/PE；分析输入是当前 logical bytes，不参与 undo / save / search 的核心 byte 语义。
- 当前没有单独的 `:asm` 命令。

## CLI 参数

| 参数 | 说明 |
|------|------|
| `--readonly` | 只读打开；需要时会自动退回只读 |
| `--offset <n\|0xhex>` | 从指定偏移开始 |
| `--pid <PID>` | 通过 PID 附加到运行中的进程进行内存编辑 |
| `--process <NAME>` | 通过进程名附加到运行中的进程进行内存编辑 |
| `--inspector` | 启动时显示 side panel 的 inspector 页 |
| `--bytes-per-line <n>` | 每行字节数，默认 `16` |
| `--page-size <n>` | 页缓存读取大小，默认 `16384` |
| `--cache-pages <n>` | 页缓存容量，默认 `128` |
| `--profile` | 退出时向 stderr 输出诊断信息 |
| `--no-color` | 禁用颜色；`NO_COLOR` 同样生效 |
| `--config <path>` | 从指定的配置文件（TOML）加载设置 |

## 配置文件

hxedit 启动时会读取一个可选的 TOML 配置文件。解析顺序（取第一个存在的）：

1. `--config <path>`
2. `$HXEDIT_CONFIG`
3. `~/.config/hxedit/config.toml`（平台配置目录）

文件不存在不会报错；文件存在但解析失败（或含未知字段）会报错退出。优先级为：CLI 参数 > 配置文件 > 内置默认值。

```toml
[display]
bytes_per_line   = 16              # 每行字节数
data_panel_bytes = 16              # data 面板解码的字节数
inspector_depth  = 1              # 此深度及更深的 struct 首次默认折叠
export_c_width   = 12              # `:export c` 每行字节数
export_py_width  = 16              # `:export py` 每块字节数
export_name      = "selection_bytes"  # `:export c`/`py` 的默认标识符

[behavior]
readonly    = false
inspector   = false                # 启动时显示 inspector 侧栏
color       = "auto"               # "auto" | "never"（"never" 等价 --no-color）
search_wrap = true                 # 搜索到边界时是否回绕到另一端

[performance]
page_size   = 16384                # 页缓存读取大小
cache_pages = 128                  # 页缓存容量
```

## 常用命令

| 命令 | 说明 |
|------|------|
| `:w` / `:w <path>` / `:wq` | 保存 / 另存 / 保存退出 |
| `:u [n]` / `:redo [n]` | undo / redo |
| `:g <offset>` / `:g end` / `:g +n` / `:g -n` | 跳转 |
| `:s [mode]<delim><pattern><delim>` / `:s! ...` | 统一搜索；默认 `/text/` 按 UTF-8 bytes 搜索，`x/hex/` 搜索原始 hex bytes，`b/255/` 搜索单字节，`u32/u64/i32/i64` 变体搜索 typed integer bytes（`!` 反向搜索）。`:S` 过渡期仅作为已废弃的 hex-search 别名保留 |
| `:p` / `:pi` / `:p?` / `:pi?` | overwrite / insert paste 与预览 |
| `:c [fmt] [disp]` | 复制当前选区 |
| `:export <path>` / `:export c` / `:export py` | 导出逻辑字节 |
| `:xor <key>` / `:xor! <key>` | 当前选区 XOR 后复制 / 原地 XOR 替换（`key`：十进制 `0..255` 或十六进制 `0x00..0xff`） |
| `:fill <pattern> <len>` / `:zero <len>` | overwrite 批量写入 |
| `:re ...` / `:re! ...` | 等长替换 / 允许长度变化的替换 |
| `:hash md5|sha1|sha256|sha512|crc32` | 哈希 |
| `:diff <path>` / `:diff -n <N> <path>` / `:diff refresh|next|prev|off` | 同步滚动显示 current logical bytes 与另一个文件；可见页会在 `N` 范围内重对齐插入/删除字节，右侧相同字节为灰色，不同字节左右亮黄，缺失字节以红色 `__` 占位 |
| `:insp` / `:insp more` | 打开 inspector / 加载更多分页项 |
| `:format ...` | 强制格式 |

`default`、`full` 或其他启用 `memory` feature 的构建下的内存编辑命令：

| 命令 | 说明 |
|------|------|
| `:mem` / `:mem list\|refresh\|info\|freeze\|thaw\|commit\|commit-all` | 打开进程内存侧面板，查看区域、刷新映射、暂停/恢复目标进程、将当前区域的替换写回（`commit`），或按虚拟地址顺序提交所有脏区域（`commit-all`）。面板有三种视图：maps（区域列表——选中/光标行和当前打开的区域分别高亮）、`:mem list` 进程选择器（Enter 附加到高亮进程）、`:mem info`（聚合报告）。所有视图支持鼠标滚轮或方向键滚动；点击行仅改变高亮 |
| `:w` / `:q` 在内存模式下 | `:w`（无路径）等同于 `:mem commit`；`:w <path>` 被拒绝（请使用 `:export <path>`）。未提交的替换、undo、redo 在区域切换时按区域保留，因此 `:q` 在有脏区域时拒绝退出并汇总总数；`:q!` 丢弃 |
| `:ms [mode]<delim><pattern><delim> [filter...]` / `:ms! ...` | 按虚拟地址搜索可读进程区域；模式包括文本、`x/hex/`、`b/byte/`、`u32/u64`，过滤器如 `in:rw-`、`in:heap`、`not:path:/usr/lib/*`、`in:va:start-end`。使用 `gn` / `gN` 重复上次内存搜索（独立于文件搜索的 `n` / `p` 历史） |

`default` / `full` 档位下的反汇编命令：

| 命令 | 说明 |
|------|------|
| `:dis [arch]` | 进入已识别 ELF / PE / Mach-O 的只读反汇编视图；文本区足够宽时显示直接分支 jump rail |
| `:dis! <arch> <offset>` | 从 display offset 强制做 raw disassembly |
| `:dis off` | 退出反汇编视图 |
| `:si` / `:si!` | 按指令文本搜索 |
| `:symbol` / `:symbol!` | 按 symbol 名搜索 |
| `:sym` / `:sym off` | 打开 / 关闭 symbol panel |
| `:data` / `:data off` | 打开 / 关闭 cursor-relative data panel |

`sagitta-analysis` 构建下的 Sagitta 分析命令：

| 命令 | 说明 |
|------|------|
| `:ana` / `:ana status` / `:ana off` | 对当前 logical bytes 运行 Sagitta、查看分析状态或清除 Sagitta snapshot。分析 ready 后覆盖 symbol panel 数据源，并为 disassembly 补充函数 label、target name 与函数体 rail；等长编辑会标记 analysis outdated，长度或布局变化后必须重新 `:ana` 才允许 Sagitta symbol 跳转。 |

## Release 产物

tag release 会按明确的 `OS * arch * feature` 矩阵发布。

当前矩阵：

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

## 许可证

本仓库中的 `hxedit` 源码以双许可发布，你可以任选其一：

- MIT（[`licenses/LICENSE-MIT`](licenses/LICENSE-MIT)）
- Apache-2.0（[`licenses/LICENSE-APACHE`](licenses/LICENSE-APACHE)）

`core` 与 `default` 构建不会启用下面单独说明的可选 Keystone 汇编依赖。

`full` 构建会启用可选的 Keystone 内联汇编依赖，并包含可选 MIT 许可的 `sagitta-rs` 分析依赖。再分发这个仓库产出的 `full` 源码包或二进制时，还应一并附带仓库内提供的第三方 notice，以及 Keystone 的 FOSS notice / license / exception 文件；见 [`licenses/THIRD_PARTY_NOTICES.txt`](licenses/THIRD_PARTY_NOTICES.txt)。

直接启用 `sagitta-analysis` 的构建也会包含 `sagitta-rs`；再分发相关 artifact 时也应保留 [`licenses/THIRD_PARTY_NOTICES.txt`](licenses/THIRD_PARTY_NOTICES.txt) 中的 Sagitta notice。
