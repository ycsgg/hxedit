# hxedit 用户指南

本指南承接 `hxedit` 的详细用户参考：feature bundle、CLI 参数、配置文件、
命令参考、release 产物和再分发说明。项目概览保留在
[README_CN.md](../README_CN.md)。

## 功能概览

- 三种明确区分的 byte 编辑操作：
  - 原地 overwrite
  - real insert
  - tombstone delete
- 编辑、粘贴、替换、inspector 写入都支持完整 undo / redo
- 统一文本 / hex / typed-value 搜索，支持前后向、自动 wrap-around、同屏命中高亮
- 大文件搜索在 clean chunk 上使用 SIMD `memmem`；只有包含 tombstone 或 replacement
  编辑的 chunk 才按需退回逐字节路径
- 内置格式检查器：ELF、PE/COFF、Mach-O、PNG、ZIP、SQLite、PCAP/PCAPNG、
  GZIP、GIF、BMP、WAV、TAR、JPEG
- Inspector 字段支持按格式定制的可读编辑器，包括 classic PCAP UTC packet
  timestamp、GZIP/PE Unix timestamp、ZIP DOS 修改时间、TAR octal
  mode/size/mtime、GIF frame delay
- 哈希：MD5、SHA1、SHA256、SHA512、CRC32
- 剪贴板复制 / 粘贴、导出、fill / zero / xor / replace
- 只读同步滚动 diff 页面，可用 `:diff` 对比另一个文件
- 进程内存编辑：通过 PID 或进程名附加到运行中的进程，浏览和编辑内存区域，
  冻结/解冻目标进程，并显式提交写回
- 可选的反汇编浏览、symbol 搜索、Sagitta 后台分析、内联汇编 patch

## 性能表现

大文件 benchmark 结果和场景说明见
[docs/performance-report.md](performance-report.md)。报告包含对外展示子集、
1 GiB save / search / diff 场景、viewport 读取、峰值 RSS，以及复现测量的命令。

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
|---|---|---|
| `core` | `cargo build --release --no-default-features` | Hex editor、inspector、search、diff、hash、copy/paste、export |
| `default` | `cargo build --release` | `core` + 进程内存编辑、反汇编视图、指令搜索、symbol panel、Rhai 脚本 |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone 驱动的内联汇编 patch + Sagitta `:ana` 分析 |
| `sagitta-analysis` 附加项 | `cargo build --release --features sagitta-analysis` | `default` + 基于 crates.io `sagitta-rs` 的 `:ana` 分析 |

说明：

- `default` 是常规构建档位，并包含进程内存编辑。
- `full` 会启用可选的 `hexpatch-keystone` 依赖（在本仓库里仍使用
  `keystone-engine` 这个本地依赖别名），开启 `:dis` 内的 inline assembly
  patch，并包含 Sagitta 分析。
- `sagitta-analysis` 启用可选的 `sagitta-rs` 分析，面向 x86/x64 ELF/PE；
  分析输入是当前 logical bytes，不参与 undo / save / search 的核心 byte 语义。
- 当前没有单独的 `:asm` 命令。

## CLI 参数

| 参数 | 说明 |
|---|---|
| `--readonly` | 只读打开；需要时会自动退回只读 |
| `--offset <n\|0xhex>` | 从指定偏移开始 |
| `--pid <PID>` | 通过 PID 附加到运行中的进程进行内存编辑 |
| `--process <NAME>` | 通过进程名附加到运行中的进程进行内存编辑 |
| `--inspector` | 启动时显示 side panel 的 inspector 页 |
| `--run <path>` | headless 执行 TOML 宏文件并退出；可重复 |
| `--script <path>` | headless 执行 Rhai 脚本并退出；在 `scripting` 构建中可用，可重复 |
| `--command <cmd>` | headless 执行可映射到执行层的命令并退出；可重复 |
| `--select display:<start>:<len>` / `--select logical:<start>:<len>` | `--run` / `--script` / `--command` 的初始 headless 选区 |
| `--bytes-per-line <n>` | 每行字节数，默认 `16` |
| `--page-size <n>` | 页缓存读取大小，默认 `16384` |
| `--cache-pages <n>` | 页缓存容量，默认 `128` |
| `--profile` | 退出时向 stderr 输出诊断信息 |
| `--no-color` | 禁用颜色；`NO_COLOR` 同样生效 |
| `--config <path>` | 从指定的配置文件（TOML）加载设置 |

当出现 `--run`、`--script` 或 `--command` 时，`hxedit` 会打开文件目标、执行自动化、
输出人类可读的 summary，然后直接退出，不创建 TUI。所有 `--run` 文件先执行，然后执行所有
`--script` 文件，最后执行所有 `--command` 字符串；三个组内各自保持传入顺序。修改只有在
宏、脚本或命令列表包含 `save`、`hx_save()`、`w` 或 `wq` 时才会写入磁盘；`:diff`、
`:insp`、`:copy`、剪贴板 paste 等 UI-only 命令会被拒绝。
headless `--command` 下，`hash`、binary `export`、`replace` 有 `--select` 时作用于该
选区，否则作用于全文件；`xor!` 必须显式提供选区。

## 配置文件

`hxedit` 启动时会读取一个可选的 TOML 配置文件。解析顺序，取第一个存在的：

1. `--config <path>`
2. `$HXEDIT_CONFIG`
3. `~/.config/hxedit/config.toml`（平台配置目录）

文件不存在不会报错。文件存在但解析失败，或含未知字段，会报错退出。优先级为：
CLI 参数 > 配置文件 > 内置默认值。

```toml
[display]
bytes_per_line   = 16              # 每行字节数
data_panel_bytes = 16              # data 面板解码的字节数
inspector_depth  = 1               # 此深度及更深的 struct 首次默认折叠
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

## 命令参考

| 命令 | 说明 |
|---|---|
| `:w` / `:w <path>` / `:wq` | 保存 / 另存 / 保存退出 |
| `:u [n]` / `:redo [n]` | undo / redo |
| `:g <offset>` / `:g end` / `:g +n` / `:g -n` | 跳转 |
| `:s [mode]<delim><pattern><delim>` / `:s! ...` | 统一搜索；默认 `/text/` 按 UTF-8 bytes 搜索，`x/hex/` 搜索原始 hex bytes，`b/255/` 搜索单字节，`u32/u64/i32/i64` 变体搜索 typed integer bytes。`!` 反向搜索。`:S` 过渡期仅作为已废弃的 hex-search 别名保留 |
| `:p` / `:pi` / `:p?` / `:pi?` | overwrite / insert paste 与预览 |
| `:c [fmt] [disp]` | 复制当前选区 |
| `:export <path>` / `:export c` / `:export py` | 导出 logical bytes |
| `:xor <key>` / `:xor! <key>` | 当前选区 XOR 后复制 / 原地 XOR 替换。`key` 可用十进制 `0..255` 或十六进制 `0x00..0xff` |
| `:fill <pattern> <len>` / `:zero <len>` | overwrite 批量写入 |
| `:re [--force] [mode]<delim><needle><delim><replacement><delim>` / `:re! ...` | 使用与 `:s` 相同的模式替换。`:re` 是等长 replacement，命中超过 65535 处时需要加 `--force` 确认；`:re!` 允许长度变化。旧的 `hex/ascii <needle> -> <replacement>` 仍兼容 |
| `:hash md5\|sha1\|sha256\|sha512\|crc32` | 哈希 |
| `:source <path>` | 执行 TOML 宏文件。宏使用显式执行层 step，可继承当前 Visual / inspector 选区，默认合并成一个 undo |
| `:script <path>` | 执行 Rhai 脚本文件。脚本使用 `hx_` host API，可继承当前 Visual / inspector 选区；除非脚本保存，否则作为一个命令入 undo |
| `:diff <path>` / `:diff -n <N> <path>` / `:diff refresh\|next\|prev\|off` | 同步滚动显示 current logical bytes 与另一个文件；可见页会在 `N` 范围内重对齐插入/删除字节，右侧相同字节为灰色，不同字节左右亮黄，缺失字节以红色 `__` 占位；`next` / `prev` 会大块分步扫描并汇报进度，扫描中阻止其它输入，Esc 取消 |
| `:insp` / `:insp more` | 打开 inspector / 加载更多分页项 |
| `:format ...` | 强制格式 |

## 宏文件

`:source <path>` 会通过与手动编辑相同的执行层执行 TOML 宏文件。首版是声明式宏：
不录制原始按键，也不运行 `:diff`、`:insp`、`:sym` 等 UI-only 命令。

同一个宏文件也可以 headless 执行：

```bash
hxedit sample.bin --run patch.hxmacro
hxedit sample.bin --select display:0x100:16 --run selection_patch.hxmacro
```

顶层字段：

| 字段 | 取值 | 说明 |
|---|---|---|
| `version` | `1` | 必填的文件格式版本 |
| `selection` | `inherit`、`clear`、`require` | 启动选区策略。TUI 下 `inherit` 使用当前 Visual / inspector 选区；headless 下除非传入 `--select`，否则初始为空 |
| `undo` | `group`、`per-step` | 整个宏合并成一个 undo step，或每个编辑 step 单独入 undo |
| `on_error` | `stop`、`rollback` | `stop` 保留已成功编辑并可 undo；`rollback` 回滚已成功编辑，并拒绝包含 `save` / `export-binary` 的宏 |

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

常用 step 形式：

| Step | 必填字段 | 效果 |
|---|---|---|
| `goto` | `offset` | 移动执行 cursor。offset 支持十进制、`0x` 十六进制、`cursor`、`cursor+N`、`cursor-N` 或 `end` |
| `select` | `space`、`start`、`len` | 设置 `display` 或 `logical` 空间的显式选区 |
| `clear-selection` | 无 | 清空执行选区 |
| `read` | `scope`；可选 `id` | 从选区或显式 range 读取 logical bytes。拒绝 `scope = "all"`，避免误把整文件物化 |
| `hash` | `algorithm`、`scope`；可选 `id` | 用 `md5`、`sha1`、`sha256`、`sha512` 或 `crc32` 哈希 |
| `search` | `pattern`；可选 `mode`、`direction`、`select`、`id` | 从当前 cursor 搜索。`select = "match"` 会选中命中范围 |
| `overwrite` | `offset`、`bytes` | replacement-only 覆盖，不移动后续 display offset |
| `insert` | `offset`、`bytes` | real insert，会移动后续 display offset，并清理不稳定选区 |
| `delete` | `scope`；可选 `kind` | 删除范围。默认 `kind = "tombstone"`，保留 display slot；`kind = "real"` 会移动 offset |
| `fill` | `offset`、`pattern`、`len` | replacement-only 重复 pattern 覆盖 |
| `xor` | `scope`、`key`、`in_place = true` | 对选区 / logical bytes 原地 XOR |
| `replace` | `scope`、`needle`、`replacement`；可选 `mode`、`allow_resize`、`force` | 替换命中内容；变长替换需要 `allow_resize = true` |
| `export-binary` | `scope`、`path` | 导出 bytes 到文件；相对路径按宏文件所在目录解析 |
| `save` | 可选 `path` | 原地保存或另存；保存成功会清空保存前 undo 历史 |

范围必须显式写出：`scope = "selection"`、`scope = "all"`，或内联 range，例如
`scope = { space = "display", start = "0x100", len = 16 }`。字节字段默认使用
hex stream，如 `"de ad be ef"`；`search` 和 `replace` 额外支持 `mode = "text"`
与 `mode = "byte"`。

会产生结果的 step 可以选择绑定 `id`。`read` 绑定选中范围的 logical bytes，
`hash` 绑定 digest 原始 bytes 和紧凑小写 hex 文本，`search` 在命中时绑定匹配
pattern bytes。后续字节字段可以引用这些值：

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

`format` 默认是 `bytes`；`hex-text` 会写入 ASCII hex。变量引用可用于
`bytes`、`pattern`、`needle`、`replacement` 等字节字段。

示例：搜索 marker，选中命中，计算 CRC32，把 CRC 的 ASCII hex 写到 marker 后面，然后保存：

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

在 TUI 中，宏执行结果会更新 cursor 和 selection。headless 模式会在请求的
macro / script / command 列表执行完后退出。除非某个 step 或后续命令显式保存，
否则编辑不会写入磁盘。

更多实际宏示例位于 [examples/](../examples/)：

| 文件 | 用途 |
|---|---|
| `firmware_header_crc.hxmacro` | 修复固定固件 header 的 CRC 和 reserved bytes |
| `extract_selected_record.hxmacro` | 导出、hash、XOR 解码并审计继承选区 |
| `sanitize_log_copy.hxmacro` | 通过 replacement 和 export 生成脱敏日志副本 |
| `strip_debug_marker.hxmacro` | 移除尾部 debug marker 并保存裁剪副本 |

## Rhai 脚本

`:script <path>` 会在 TUI 中运行 Rhai 脚本，`--script <path>` 会 headless 运行同一类脚本。
两条路径都使用同一执行层。TUI 脚本会继承当前 Visual / inspector 选区；除非脚本调用
`hx_save()`，否则脚本编辑会作为一个 undo step。Rhai scripting 包含在 `default` 和
`full` 构建中；`core` / `--no-default-features` 构建会对 `:script` 和 `--script`
返回 feature 错误。

脚本 API 只暴露执行层操作，并统一使用 `hx_` 前缀：

| 函数 | 返回值 | 说明 |
|---|---|---|
| `hx_hex(text)` | bytes blob | 解析 hex stream，例如 `"de ad be ef"` |
| `hx_ascii(text)` | bytes blob | 把文本转成原始 bytes |
| `hx_cursor()` | integer | 当前 display cursor |
| `hx_len_display()` | integer | 当前 display 长度 |
| `hx_len_logical()` | integer | 当前 logical byte 长度 |
| `hx_goto(offset)` | 无 | 跳到绝对 display offset |
| `hx_goto_end()` | 无 | 跳到最后一个 display offset；空文件为 `0` |
| `hx_select_display(start, len)` | 无 | 设置 display-space 选区 |
| `hx_select_logical(start, len)` | 无 | 设置 logical-space 选区 |
| `hx_clear_selection()` | 无 | 清空当前选区 |
| `hx_has_selection()` | bool | 当前是否有选区 |
| `hx_selection_start()` | integer | 当前选区起点；无选区时报错 |
| `hx_selection_len()` | integer | 当前选区长度；无选区时报错 |
| `hx_selection_space()` | string | 当前选区空间：`"display"` 或 `"logical"` |
| `hx_read_display(start, len)` | bytes blob | 读取 display range 覆盖的 logical bytes |
| `hx_read_logical(start, len)` | bytes blob | 读取 logical range 覆盖的 logical bytes |
| `hx_read_selection()` | bytes blob | 读取当前选区的 logical bytes |
| `hx_search(bytes)` | integer | `hx_search_forward(bytes)` 的兼容别名 |
| `hx_search_forward(bytes)` / `hx_search_backward(bytes)` | integer | 从当前 cursor 搜索；命中返回 display offset，失败返回 `-1` |
| `hx_search_forward_select(bytes)` / `hx_search_backward_select(bytes)` | integer | 搜索并把命中 display range 设为选区 |
| `hx_hash_hex(algorithm)` | string | 有选区时 hash 当前选区；没有选区时 hash 整个文件 |
| `hx_hash_display_hex(start, len, algorithm)` | string | hash display range |
| `hx_hash_logical_hex(start, len, algorithm)` | string | hash logical range |
| `hx_hash_selection_hex(algorithm)` | string | hash 当前选区；无选区时报错 |
| `hx_hash_all_hex(algorithm)` | string | hash 当前全部 logical bytes |
| `hx_overwrite(offset, bytes)` | 无 | 在 display offset 做 replacement-only 覆盖 |
| `hx_insert(offset, bytes)` | 无 | 在 display offset 做 real insert |
| `hx_fill(offset, pattern, len)` | 无 | replacement-only 重复 pattern 覆盖 |
| `hx_delete_display(start, len)` / `hx_delete_logical(start, len)` / `hx_delete_selection()` | 无 | tombstone delete；display slot 保留，logical bytes 消失 |
| `hx_delete_real_display(start, len)` / `hx_delete_real_logical(start, len)` / `hx_delete_real_selection()` | 无 | real delete；后续 display offset 左移 |
| `hx_xor_display(start, len, key)` / `hx_xor_logical(start, len, key)` / `hx_xor_selection(key)` | 无 | replacement-only 原地 XOR；`key` 为 `0..255` |
| `hx_replace_all(needle, replacement, allow_resize, force)` | 无 | 在当前全部 logical bytes 中替换匹配 |
| `hx_replace_display(start, len, needle, replacement, allow_resize, force)` | 无 | 在 display range 中替换匹配 |
| `hx_replace_logical(start, len, needle, replacement, allow_resize, force)` | 无 | 在 logical range 中替换匹配 |
| `hx_replace_selection(needle, replacement, allow_resize, force)` | 无 | 在当前选区中替换匹配 |
| `hx_export_display(start, len, path)` / `hx_export_logical(start, len, path)` / `hx_export_selection(path)` | 无 | 导出 logical bytes 到二进制文件 |
| `hx_save()` | 无 | 保存当前 document |
| `hx_save_as(path)` | 无 | 另存为指定路径 |

`hx_export_*` 和 `hx_save_as()` 的相对路径从脚本文件所在目录解析。
`allow_resize = false` 保持 replacement-only 替换；`allow_resize = true` 允许 real
delete / insert，并会清空选区。

默认脚本预算为 `2,000,000` 次 Rhai operation、`100,000` 次执行层调用、`512 MiB`
脚本读取总量、`64 MiB` 单次读取、`64 MiB` 单个 bytes blob。hash/search/export
这类操作仍走 streaming document 路径；除非脚本显式调用 `hx_read_*`，不会把整文件物化进
VM。

脚本示例：

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

headless 执行：

```bash
hxedit sample.bin --script examples/simple_hash_patch.hxscript
```

TUI 命令行中使用：

```text
:script examples/simple_hash_patch.hxscript
```

headless command list 可以混合脚本和普通执行层命令：

```bash
hxedit sample.bin --command "script patch.hxscript" --command w
```

固定步骤、声明式流程优先用宏；需要分支、循环，或后续 offset / bytes 依赖前面
search、read、hash 结果时，用脚本更合适。

更多实际脚本示例位于 [examples/](../examples/)：

| 文件 | 用途 |
|---|---|
| `simple_hash_patch.hxscript` | 给 marker 邻近字段写入 CRC32 |
| `extract_payload_between_markers.hxscript` | 导出两个 marker 之间的 payload 并写入 SHA-256 |
| `decode_selected_xor.hxscript` | 解码继承的 XOR 选区并保存审计产物 |
| `sanitize_log_copy.hxscript` | 脱敏常见日志 token 并归一化换行 |
| `trim_debug_trailer.hxscript` | 在保存副本中移除 debug / volatile marker |

`default`、`full` 或其他启用 `memory` feature 的构建下的内存编辑命令：

| 命令 | 说明 |
|---|---|
| `:mem` / `:mem list\|refresh\|info\|freeze\|thaw\|commit\|commit-all` | 打开进程内存侧面板，查看区域、刷新映射、暂停/恢复目标进程、将当前区域的 replacement span 写回（`commit`），或按虚拟地址顺序提交所有脏区域（`commit-all`）。面板有 maps、process list 和 info 三种视图。所有视图支持鼠标滚轮或方向键滚动；点击行仅改变高亮 |
| `:w` / `:q` 在内存模式下 | `:w`（无路径）等同于 `:mem commit`；`:w <path>` 被拒绝（请使用 `:export <path>`）。未提交的 replacement、undo、redo 在区域切换时按区域保留，因此 `:q` 在有脏区域时拒绝退出并汇总总数；`:q!` 丢弃 |
| `:ms [mode]<delim><pattern><delim> [filter...]` / `:ms! ...` | 按虚拟地址搜索可读进程区域；模式包括文本、`x/hex/`、`b/byte/`、`u32/u64`，过滤器如 `in:rw-`、`in:heap`、`not:path:/usr/lib/*`、`in:va:start-end`。使用 `gn` / `gN` 重复上次内存搜索，独立于文件搜索的 `n` / `p` 历史 |

`default` / `full` 档位下的反汇编命令：

| 命令 | 说明 |
|---|---|
| `:dis [arch]` | 进入已识别 ELF / PE / Mach-O 的只读反汇编视图；文本区足够宽时显示直接分支 jump rail |
| `:dis! <arch> <offset>` | 从 display offset 强制做 raw disassembly |
| `:dis off` | 退出反汇编视图 |
| `:si` / `:si!` | 按指令文本搜索 |
| `:symbol` / `:symbol!` | 按 symbol 名搜索 |
| `:sym` / `:sym off` | 打开 / 关闭 symbol panel |
| `:data` / `:data off` | 打开 / 关闭 cursor-relative data panel |

`sagitta-analysis` 构建下的 Sagitta 分析命令：

| 命令 | 说明 |
|---|---|
| `:ana` / `:ana status` / `:ana off` | 对当前 logical bytes 运行 Sagitta、查看分析状态或清除 Sagitta snapshot。分析 ready 后覆盖 symbol panel 数据源，并为 disassembly 补充函数 label、target name 与函数体 rail；等长编辑会标记 analysis outdated，长度或布局变化后必须重新 `:ana` 才允许 Sagitta symbol 跳转 |

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

## Redistribution Notes

本仓库中的 `hxedit` 源码以双许可发布，你可以任选其一：

- MIT（[licenses/LICENSE-MIT](../licenses/LICENSE-MIT)）
- Apache-2.0（[licenses/LICENSE-APACHE](../licenses/LICENSE-APACHE)）

`core` 与 `default` 构建不会启用下面单独说明的可选 Keystone 汇编依赖。

`full` 构建会启用可选的 Keystone 内联汇编依赖，并包含可选 MIT 许可的
`sagitta-rs` 分析依赖。再分发这个仓库产出的 `full` 源码包或二进制时，还应一并附带
仓库内提供的第三方 notice，以及 Keystone 的 FOSS notice / license / exception 文件；
见 [licenses/THIRD_PARTY_NOTICES.txt](../licenses/THIRD_PARTY_NOTICES.txt)。

直接启用 `sagitta-analysis` 的构建也会包含 `sagitta-rs`；再分发相关 artifact 时也应保留
[licenses/THIRD_PARTY_NOTICES.txt](../licenses/THIRD_PARTY_NOTICES.txt) 中的 Sagitta notice。

`default` / `full` 构建包含 `scripting` feature 和 `MIT OR Apache-2.0` 许可的 Rhai
依赖；再分发相关 artifact 时也应保留
[licenses/THIRD_PARTY_NOTICES.txt](../licenses/THIRD_PARTY_NOTICES.txt) 中的 Rhai notice。
