# Architecture & Current Snapshot

本文档只保留**当前仍有用**的实现快照、产品定位、行为边界、代码地图与 CI/Release 状态。
已经完成的历史整理项交给 git history，不再在这里长期堆积。

---

## 0. 当前快照

### 产品定位

`hxedit` 是一个面向**大文件**的 TUI hex editor。优先级始终是：

1. byte 级编辑语义正确
2. undo / save / search 稳定
3. 大文件性能可接受
4. 文档与实现一致

### 当前已落地的用户面

- 编辑模型：overwrite / insert / tombstone delete / real delete（仅特定路径）/ undo / redo
- 搜索：统一 `:s [mode]<delim><pattern><delim>` / `:s! ...` 入口覆盖 UTF-8 bytes、hex bytes、单字节与整数 typed-value，支持前后向、自动 wrap-around、同屏命中高亮；`:S` 过渡期仅作为 deprecated hex-search 别名保留。底层 `search_forward` / `search_backward` 用 SIMD `memchr::memmem` 加速：clean 文档全程 memmem；dirty 文档（tombstone / replacement）仍逐 chunk 判脏净，**clean chunk 也走 memmem**（`scan_clean_chunk_forward` / `_backward`，首尾 `P-1` 字节用共享 `KmpMatcher` 衔接跨 chunk/piece 边界），只有含编辑的 chunk 才逐字节 KMP。实测 256MB worst-case：clean ~53ms、dirty(单 tombstone) ~38ms（旧版全 KMP ~330ms，约 8× 提升）。`default` / `full` 额外支持进程内存搜索、指令文本与 symbol 搜索；`memory` feature 下 `:ms` / `:ms!` 跨 region 搜索用同类 mode 前缀语法与 `memchr::memmem`，命中后 `gn` / `gN` 重放（独立于文件 `n` / `p`）
- 主 hex 视图顶部固定显示 byte 列头（默认 `00 01 02 ... 0F`），列头不随 viewport 滚动；鼠标命中与 `view_rows` 只计算列头下方的数据行
- 选区：Visual 选区，或 inspector 当前字段范围作为 active selection
- 命令：`:g`、`:hash`、`:xor` / `:xor!`、`:re` / `:re!`、`:fill`、`:zero`、`:export`、`:diff`、`:insp more`、`:dis`、`:sym`、`:data`；`default` / `full` 内置 `memory` feature，支持 `:mem freeze` / `:mem thaw` 暂停与恢复目标进程，`:mem commit` / `:mem commit-all` 写回 replacement（后者按 VA 升序遍历所有 dirty region），`:w` 等价 `:mem commit` 且 `:w <path>` 被拒绝（改用 `:export`）；`MemorySession` 按 region 持久化未提交 replacement / undo / redo，切换 region 不丢失编辑，`:q` 在任意 region dirty 时拒绝并汇总；`sagitta-analysis` feature 下额外支持 `:ana` / `:ana status` / `:ana off`，对当前 logical bytes 后台运行 crates.io `sagitta-rs` 并用 ready snapshot 覆盖 symbol panel 数据源
- Inspector / side panel：
  - Inspector：ELF、PE/COFF、Mach-O、PNG、ZIP（central directory / EOCD / ZIP64 / data descriptor 感知）、SQLite（database header / b-tree page header / cell pointer array，不深入 record payload）、PCAP / PCAPNG（capture/header/block/packet data range，不深入链路层 payload）、GZIP、GIF、BMP、WAV、TAR、JPEG
  - Symbol panel：可执行文件 symbol / import 列表与跳转；`sagitta-analysis` ready 后使用 Sagitta recovered functions 覆盖 native entries
  - Data panel：cursor-relative primitive decode
- Disassembly：
  - `default`：Capstone 驱动的只读反汇编浏览
  - `full`：在 `default` 基础上开放 Keystone inline assemble patch
  - `sagitta-analysis`：可选后台分析层，只读消费 current logical bytes，ready snapshot 覆盖 symbol panel，并为反汇编行补充函数入口 label、direct branch target 名称与函数体 rail
- 保存：当前统一走 rewrite-save；同路径保存保留权限位；save-as 允许从 readonly 文档写到新路径
- 配置：启动时读取可选 TOML 配置文件（`--config` > `$HXEDIT_CONFIG` > `~/.config/hxedit/config.toml`）。优先级 CLI 参数 > 配置文件 > 内置默认值；文件缺失静默用默认，存在但解析失败/含未知字段则报错退出。当前覆盖 `[display]`（bytes_per_line / data_panel_bytes / inspector_depth / export_c_width / export_py_width / export_name）、`[behavior]`（readonly / inspector / color=auto\|never / search_wrap）、`[performance]`（page_size / cache_pages）。runtime `Config` 仍是扁平结构，三段式仅是磁盘 TOML 布局（`FileConfig`）
- 许可 / 发布：仓库自身源码当前以 `MIT OR Apache-2.0` 双许可发布；`full` 档位当前使用可选 `hexpatch-keystone` 依赖（代码内仍保留 `keystone-engine` 依赖别名）并包含 `sagitta-analysis`，release artifact 需要附带 `licenses/THIRD_PARTY_NOTICES.txt`、`licenses/keystone/FOSS-NOTICE.txt`，以及 Keystone 的 license / exception 文件，不能把 `full` 二进制简单写成 `MIT/Apache-only`

### 当前明确的行为边界

- Tab 是 side panel 可见性开关：panel 隐藏时打开并进入 panel；panel 已显示时关闭并回到 Normal，不作为 Normal 与 panel 的焦点循环键
- overwrite paste / `:fill` / `:zero` 越过 EOF 会截断，不会自动 append
- `:xor` 只读取 active selection 的 logical bytes，XOR 后以 hex 文本复制到剪贴板；`:xor!` 是 replacement 语义的原地覆盖，不改变 piece 布局；key 不带 `0x` 时按十进制解析，带 `0x` 时按 hex 解析
- insert paste 会真实插入并右移后续 display offset
- PNG / ZIP inspector 可编辑字段只代表“能写 byte”，**不代表结构安全**
- `save` / `logical_bytes()` / `read_logical_range()` / `:hash` / `:export`(binary, `for_each_logical_chunk`) / dirty search / diff current-source / `:xor!`(`transform_visible_range_in_place`) 统一经 `src/core/document/walk.rs` 走 piece-walking + 分块 overlay：tombstone、replacement、Original/Add 的组合语义只在 walker 中判定；读原始文件时优先按 `Document::max_contiguous_read_len()`（约等于 `page_size * cache_pages`）裁剪，避免小 cache 配置下过度换页
- `:diff` 是只读同步滚动投影视图：current side 使用当前文档 logical bytes（跳过 tombstone、应用 replacement、包含 insert），other side 使用对比文件 raw bytes；不写入 `Document`，不进 undo/save；打开时只读打开 other 文件，不做全文件预扫描，render 时只读取当前可见页并在 `max_shift` 范围内局部重对齐后着色。着色约定：相同字节只在右侧灰色；同位置不同字节左右均 warning 亮黄色；current-only / other-only 的缺失侧补 `__` 并用 error 红色。other-only `__` 是投影 cell，需要参与鼠标 hit-test / 选区映射；切走或隐藏 diff panel 后必须停止投影，左侧不保留 `__` 或 diff 着色。

### CI / Release

- Rust 固定为 `1.94.1`
- GitHub Actions 在 Ubuntu / Windows 跑：
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --all-targets`
- CI 覆盖 `core` / `default` / `full` 三个 feature 档位
- release 当前按 `OS * arch * feature` 矩阵构建：
- 所有 release artifact 都附带双许可文件；`full` artifact 额外附带 `licenses/keystone/FOSS-NOTICE.txt`、第三方 notice / 许可证 / exception 文本
  - Linux x86_64 / aarch64
  - macOS arm64
  - Windows x86_64
  - 每个平台均产出 `core` / `default` / `full`

---

## 1. 代码地图与高风险路径

| 模块 | 责任 | 改动时重点检查 |
|---|---|---|
| `src/core/document/*` | 文档读写、编辑、搜索、保存入口 | display / visible / logical 语义是否仍一致 |
| `src/core/piece_table.rs` | 真实插入 / 真实删除、`CellId` 稳定性 | split / merge / restore 是否破坏 id 稳定性 |
| `src/app/editing_state.rs` | nibble 编辑、插入、删除 | mode 切换、EOF 行为、undo 记录 |
| `src/app/mode_state.rs` | mode 切换、selection range | Visual / Inspector / Command 返回路径 |
| `src/app/clipboard_ops.rs` | copy / paste / preview | overwrite vs insert、display span vs logical bytes |
| `src/diff/*` / `src/app/diff_state.rs` | 只读 diff source / engine / panel 状态 | current logical bytes vs other raw bytes、可见页即时读取、不要写 Document、不要打开时全文件扫描 |
| `src/app/commands.rs` / `src/app/commands/*` | `:` 命令分发与按领域执行 | 状态栏反馈、active selection、readonly 处理；拆分时只移动代码，不混入语义改动 |
| `src/app/events.rs` / `src/app/events/*` | action 分发、命令输入、编辑入口、side panel / inspector 事件收尾 | mode 返回路径、EOF clamp、错误状态清理；输入状态机不要重新堆回单文件 |
| `src/app/inspector_state.rs` | inspector refresh / edit / fold / scroll | `NodePath` 稳定性、field 定位、格式丢失反馈 |
| `src/app/undo.rs` | undo / redo 回放 | real delete / tombstone / replacement 是否混写 |
| `src/view/status.rs` / `src/app/render.rs` / `src/app/render/*` | 状态栏、主视图、side panel 与 overlay 渲染 | cursor / selection / search / inspector 高亮能否叠加；hex 固定列头是否仍不参与滚动 / 命中测试；diff 投影仍只读 |
| `src/format/*` | detect / parse / edit | “可编辑”不等于“结构安全”；大结构要考虑分页 |

---

## 2. 当前仍需持续关注的点

### 2.1 文档漂移

当前文档已经比以前收敛，但仍要持续避免：

- README 写的是旧命令面（尤其搜索统一入口与 deprecated alias）
- `docs/issues.md` 被已完成事项占满
- `commands/hints.rs`、测试名、状态栏文案与实现脱节
- `:diff` 文案必须继续写明 current side 是 logical bytes，不要退回成 display offset 对比

### 2.2 App 层回归覆盖仍不够深

已有基础测试，但高风险路径仍建议继续加：

- paste（overwrite / insert / preview）
- visual delete
- inspector edit
- undo / redo
- mode clamp / command return mode

### 2.3 Inspector 仍是全量刷新

当前编辑后通常仍是 detect → parse → flatten → rebuild rows。实现简单可靠，但若以后开始支持更深的结构树或大表格，需要重新评估。

### 2.4 结构安全仍需保守

- ELF 某些头字段可编辑，风险相对可控
- PNG / ZIP 仍不能自动修 CRC、descriptor、目录索引等一致性；ZIP inspector 已能读取 central directory / EOCD / ZIP64 / data descriptor，但编辑仍只是 byte 写入
- 新增 editable 字段时，宁可少开，也不要把“能改字节”误写成“结构安全”

### 2.5 格式支持深度仍有限

虽然当前已支持 ELF / PE / Mach-O / PNG / ZIP / SQLite / PCAP / PCAPNG / GZIP / GIF / BMP / WAV / TAR / JPEG，但仍有深度边界：

- ZIP 已从 local-header partial scan 推进到 central directory / EOCD / ZIP64 / data descriptor 感知，但还没有结构间跳转或自动一致性修复
- SQLite 当前只做轻量容器级解析：database header、b-tree page header、cell pointer array 与 cell content data range；不解码 record payload / schema / SQL 层语义
- PCAP / PCAPNG 当前只做 capture 容器级解析：global header、packet records、section/interface/packet blocks 与 packet data range；不解码 Ethernet/IP/TCP/UDP 等链路层或网络层 payload
- 某些格式仍以“安全浏览 + 保守编辑”为主，不做结构修复
- 大表格 / 深层结构仍需要继续补分页、跳转与更细粒度视图

### 2.6 Diff view 仍必须保持“只读投影”定位

- `:diff` 的设计见 `diff-design.md`
- Diff hunk 不能被解释成 real delete / tombstone / replacement；它只是 current logical stream 与 other raw stream 的比较结果
- current-side overlay 由可见 display slot 映射到 logical offset 后即时生成；有 tombstone 时不要假设 logical range 等于连续 display range
- 局部重对齐投影中的 other-only `__` 没有 current display slot，但仍占一个可见 cell；左侧点击时锚到相邻 current display slot，右侧 diff panel 点击时同步左侧光标并高亮对应 other raw byte
- diff 投影只在 `show_side_panel && active_side_panel == Diff && diff_state.is_some()` 时启用；Tab 隐藏或 `:data` / `:insp` / `:sym` 切到其他 side panel 时要清掉 diff panel 投影态，避免主视图残留 `__` / 着色
- 打开 `:diff` 只读打开 other 文件，不能全文件预扫描；编辑 / undo / redo / save reload 后可直接按当前可见页重新着色，不应自动大文件重扫

### 2.7 App 大文件拆分后的边界

- `src/app/render.rs` 只保留共享 render 类型、小 helper 与模块声明；主视图、diff 投影、side panel 和 render 测试分别放在 `src/app/render/*`
- `src/app/events.rs` 只保留 `handle_action` 入口；分发、编辑入口、inspector edit 与 action 收尾分别放在 `src/app/events/*`
- `src/app/commands.rs` 只保留 `submit_command` 与 command match 分发；文件导航、transform、inspector、search/disasm、hash/diff、symbols、memory 分别放在 `src/app/commands/*` 或所属状态模块
- 后续继续拆分时，先做 move-only / helper 提取并跑相关测试，不要把结构移动和 real delete / tombstone / replacement 语义调整混在同一阶段

### 2.8 已落地的 disassembly 模式仍必须保持“view 层”定位

- 可执行文件反汇编设计见 `disassemble-design.md`
- `:dis` 应是主视图切换，不应把 `Mode` 直接扩成 instruction 语义状态机
- 反汇编结果始终只是当前 bytes 的投影，不应改写 `Document` 的 real delete / tombstone / replacement 语义
- 当前已经实现 backend 抽象、`CapstoneBackend`、symbol panel、data panel 与 `full` 档位下的 Keystone inline patch；后续继续扩展时仍不要把 App / render / cache 逻辑绑死到单一 backend API
- `sagitta-analysis` 走 `src/app/analysis_state.rs` 的后台线程 + mpsc result channel，worker 只持有物化后的 logical bytes / job id / revision / sender；主线程安装 owned snapshot。编辑 invalidation 只标记 `Current` / `OutdatedBytes` / `InvalidLayout`，不把 Sagitta 写入 `Document`

### 2.9 Assembler backend 维护风险仍需持续关注

- 当前 `full` 使用 `hexpatch-keystone` 路线来减轻 crates.io / 新工具链上的构建问题，但这不等于 Keystone 生态本身已经足够现代或低风险
- 后续仍应评估更可维护的 backend（如 `iced-x86` / `zydis` / `asmjit` 等），或在只覆盖少量简单 patch 指令时直接维护最小自研后端

### 2.10 性能基准与 correctness 回归应分流

- 性能观测已迁移到 `cargo bench --bench perf_bench`，包含 16MB/64MB save 场景、hash、logical_bytes、paste overwrite、piece lookup、ELF parse、当前 search 路径；日常 `cargo test` 只保留 correctness-first 的回归断言，不设 wall-clock 阈值

### 2.11 crates.io 发布自动化仍待补齐

- 当前已能走本地 `cargo package` / `cargo publish --dry-run`
- 但真正发布仍偏手工；后续应补齐 tag/版本一致性检查、dry-run、正式发布与失败日志的 CI/CD 流程
