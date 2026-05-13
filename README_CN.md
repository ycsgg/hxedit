# hxedit

面向大文件的终端十六进制编辑器，使用 Rust 编写。

主 README 在这里：[README.md](README.md)

`hxedit` 优先保证 byte 级编辑语义正确，提供非破坏式编辑、完整 undo/redo、搜索、格式检查器，以及可选的可执行文件反汇编浏览。

## 功能

- 三种明确区分的 byte 编辑操作：
  - 原地 overwrite
  - real insert
  - tombstone delete
- 编辑、粘贴、替换、inspector 写入都支持完整 undo / redo
- ASCII / hex 搜索，支持前后向、自动 wrap-around、同屏命中高亮
- 内置格式检查器：ELF、PE/COFF、Mach-O、PNG、ZIP、GZIP、GIF、BMP、WAV、TAR、JPEG
- 哈希：MD5、SHA1、SHA256、SHA512、CRC32
- 剪贴板复制 / 粘贴、导出、fill / zero / xor / replace
- 只读同步滚动 diff 页面，可用 `:diff` 对比另一个文件
- 分页 I/O 和缓存，适合大文件
- 可选的反汇编浏览、symbol 搜索、内联汇编 patch

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
| `default` | `cargo build --release` | `core` + 反汇编视图、指令搜索、symbol panel |
| `full` | `cargo build --release --no-default-features --features full` | `default` + Keystone 驱动的内联汇编 patch |

说明：

- `default` 是常规构建档位。
- `full` 会 vendor `keystone-engine`，并开启 `:dis` 内的 inline assembly patch。
- 当前没有单独的 `:asm` 命令。

## CLI 参数

| 参数 | 说明 |
|------|------|
| `--readonly` | 只读打开；需要时会自动退回只读 |
| `--offset <n\|0xhex>` | 从指定偏移开始 |
| `--inspector` | 启动时显示 side panel 的 inspector 页 |
| `--bytes-per-line <n>` | 每行字节数，默认 `16` |
| `--page-size <n>` | 页缓存读取大小，默认 `16384` |
| `--cache-pages <n>` | 页缓存容量，默认 `128` |
| `--profile` | 退出时向 stderr 输出诊断信息 |
| `--no-color` | 禁用颜色；`NO_COLOR` 同样生效 |

## 常用命令

| 命令 | 说明 |
|------|------|
| `:w` / `:w <path>` / `:wq` | 保存 / 另存 / 保存退出 |
| `:u [n]` / `:redo [n]` | undo / redo |
| `:g <offset>` / `:g end` / `:g +n` / `:g -n` | 跳转 |
| `:s <text>` / `:s! <text>` | ASCII 搜索 |
| `:S <hex>` / `:S! <hex>` | Hex 搜索 |
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

`default` / `full` 档位下的反汇编命令：

| 命令 | 说明 |
|------|------|
| `:dis [arch]` | 进入已识别 ELF / PE / Mach-O 的只读反汇编视图 |
| `:dis! <arch> <offset>` | 从 display offset 强制做 raw disassembly |
| `:dis off` | 退出反汇编视图 |
| `:si` / `:si!` | 按指令文本搜索 |
| `:symbol` / `:symbol!` | 按 symbol 名搜索 |
| `:sym` / `:sym off` | 打开 / 关闭 symbol panel |
| `:data` / `:data off` | 打开 / 关闭 cursor-relative data panel |

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

`hxedit` 以 `GPL-2.0-only` 发布。
