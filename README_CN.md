# hxedit

面向大文件的终端十六进制编辑器，快速、可靠。

English README: [README.md](README.md)

`hxedit` 让你在终端里打开、浏览、编辑、搜索、对比、哈希和导出二进制数据。它通过分页
缓存读取文件， GB 级的大文件也能快速打开，并且把每一次字节编辑都保持显式，让 undo、save
和 search 的行为始终可预测。

![hxedit 主视图](docs/images/main_view.png)

*带可移动光标的分页 hex/ASCII 视图、随格式变化的 inspector 侧边面板，以及底部的命令行和
当前模式。*

## 安装

从 crates.io 安装最新发布版：

```bash
cargo install hxedit
```

或从 [releases 页面](https://github.com/ycsgg/hxedit/releases) 下载预编译二进制，解压后把
`hxedit` 放到 `PATH` 里。

然后打开文件：

```bash
hxedit some.bin
```

以只读方式从指定 offset 打开，并显示 inspector：

```bash
hxedit --readonly --offset 0x100 --inspector some.bin
```

## 按键

`hxedit` 采用类似 vim 的模式化操作。按 `:` 执行命令，按 `Esc` 回到 normal 模式。

| 按键                    | 操作                                         |
| --------------------- | ------------------------------------------ |
| `h` `j` `k` `l` / 方向键 | 移动光标；`PageUp`/`PageDown`、`Home`/`End` 按行移动 |
| `i`                   | 进入 insert 模式（输入的 hex 会向后移动数据）              |
| `r`                   | 进入 overwrite 模式（输入的 hex 原地覆盖字节）            |
| `x`                   | 删除光标处的字节（或选区）                              |
| `v`                   | 开始 / 结束 visual 选区                          |
| `n` / `p`             | 跳到下一个 / 上一个搜索结果                            |
| `t` 或 `Tab`           | 切换侧边面板（inspector / 内存 等）                   |
| `:`                   | 打开命令行                                      |
| `Esc`                 | 退出当前模式                                     |
| `Ctrl+Z` / `Ctrl+Y`   | 编辑时 undo / redo                            |
| `Ctrl+C`              | 强制退出                                       |

## 命令

| 命令                                | 说明                    |
| --------------------------------- | --------------------- |
| `:w` / `:wq`                      | 保存 / 保存并退出            |
| `:q` / `:q!`                      | 退出 / 放弃修改并退出          |
| `:u [n]` / `:redo [n]`            | undo / redo           |
| `:g <offset>` / `:g end`          | 跳转到 offset            |
| `:s /text/` / `:s x/de ad be ef/` | 搜索文本或 hex bytes       |
| `:p` / `:pi`                      | overwrite / insert 粘贴 |
| `:c [fmt]`                        | 复制当前选区                |
| `:export <path>`                  | 导出当前编辑后的字节            |
| `:hash sha256`                    | 对选区或整个文件求哈希           |
| `:source <path>`                  | 执行 TOML 宏文件             |
| `:diff <path>`                    | 对比另一个文件               |
| `:insp`                           | 打开格式 inspector        |

完整命令语法、CLI 参数、配置文件、内存编辑、反汇编和 Sagitta 分析见
[用户指南](docs/user-guide_CN.md)。

## 能做什么

- 用 overwrite、insert、delete 打开和编辑大文件

- 搜索文本、hex bytes、单字节值或 typed integer

- 对选区执行 copy、export、hash、fill、zero、XOR 或 replace

- 通过与手动编辑相同的执行层运行 TOML 宏文件

- 直接查看 ELF、PE/COFF、Mach-O、PNG、ZIP、SQLite、PCAP、GZIP、GIF、BMP、WAV、
  TAR 和 JPEG 的结构

- 用只读同步 diff 视图和另一个文件对比

- 可选进行进程内存编辑、反汇编浏览、symbol 查询、Sagitta 分析和内联汇编 patch

![hxedit 反汇编视图](docs/images/dis_view.png)

*带分支跳转轨道的只读反汇编视图（`:dis`）。*

![hxedit Sagitta 分析视图](docs/images/ana_view.png)

*基于当前 logical bytes 的 Sagitta 分析（`:ana`）。*

![hxedit diff 视图](docs/images/diff_view.png)

*和另一个文件的只读同步 diff（`:diff`）。*

## 性能

`hxedit` 是为大文件设计的。在 1 GiB 文件上，打开远小于 1 毫秒，端到端搜索约 190 ms，
同步 diff 遍历约 320 ms，同时峰值 RSS 保持在个位数 MiB。完整场景、硬件和复现命令见
[docs/performance-report.md](docs/performance-report.md)。

## 构建档位

| 档位        | 命令                                                            | 适合场景                                              |
| --------- | ------------------------------------------------------------- | ------------------------------------------------- |
| `core`    | `cargo build --release --no-default-features`                 | 只需要编辑器、inspector、搜索、diff、hash、copy/paste 和 export |
| `default` | `cargo build --release`                                       | 常规构建，包含进程内存编辑、反汇编和 symbol                         |
| `full`    | `cargo build --release --no-default-features --features full` | 还需要 Keystone 内联汇编 patch 和 Sagitta 分析              |

## 需要知道的行为

字节编辑是显式的：overwrite 原地改字节，insert 会移动后续数据，delete 在保存或导出前
都是非破坏性的。这正是 undo、save、search、export、hash、diff 和 inspector 写入保持一致的
原因。格式 inspector 只写入你编辑的字节，不会自动帮你修复 checksum、CRC 或布局。进程内存
编辑在你显式 commit 回目标进程之前一直是本地的。准确语义见
[docs/editing-model.md](docs/editing-model.md)。

## 文档

| 文档                                                       | 用途                    |
| -------------------------------------------------------- | --------------------- |
| [docs/user-guide\_CN.md](docs/user-guide_CN.md)          | 面向用户的构建、CLI、配置和命令参考   |
| [docs/performance-report.md](docs/performance-report.md) | 大文件 benchmark 场景和复现命令 |
| [docs/architecture.md](docs/architecture.md)             | 当前产品面、行为边界和代码地图       |

## 许可证

本仓库中的 `hxedit` 源码以 MIT 或 Apache-2.0 双许可发布，你可以任选其一。完整文本和
第三方 notice 见 [licenses/](licenses/)。

`full` 构建会启用可选 Keystone 内联汇编 patch 和 Sagitta 分析。再分发 `full`
artifact 时必须附带
[docs/user-guide\_CN.md](docs/user-guide_CN.md#redistribution-notes) 中说明的第三方与
Keystone notice 文件。
