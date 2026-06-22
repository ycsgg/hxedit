# Archived: Completed Items

本文件归档 `docs/issues.md` 中已完成（`[x]`）的事项与历史 baseline，仅作记录。
当前仍有行动价值的 backlog 见 `docs/issues.md`。

---

## Current Baseline（已落地）

- [x] Rust 与 CI 基线已固定：`1.94.1` + `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test --all-targets`
- [x] 当前对外命令面已经覆盖 `:g`、`:hash`、`:xor` / `:xor!`、`:re` / `:re!`、`:fill`、`:zero`、`:export`、`:diff`、`:insp more`、`:dis`、`:sym`、`:data`
- [x] 搜索支持 forward / backward / wrap-around，并在 hex grid 对同屏命中做叠加高亮
- [x] Inspector 当前支持 ELF / PE/COFF / Mach-O / PNG / ZIP / SQLite / PCAP / PCAPNG / GZIP / GIF / BMP / WAV / TAR / JPEG
- [x] ELF inspector 已扩展到 section header / section data / dynamic entries / interpreter / notes / GNU property / string table / symbol table / relocation / SysV & GNU hash / version sections，并接入 `:insp more` 分页
- [x] `collapsed_nodes` 已改为 `NodePath`，折叠态不再依赖脆弱的前序 `node_id`
- [x] save 当前统一走 rewrite path；同路径保留权限位；hash / logical export 已走分块读取
- [x] `default` / `full` 已提供 executable metadata detect、read-only disassembly、symbol search、symbol panel、data panel
- [x] `:diff` 已落地为只读同步滚动 side panel，使用 current logical bytes vs other raw bytes，可见页局部重对齐后即时着色，打开不做全文件预扫描，支持 50/50 layout 与 hex overlay；相同字节右侧灰色、不同字节左右 warning 亮黄、缺失侧补 `__` 并以 error 红色标注；other-only `__` 参与鼠标 hit-test / 选区映射，左右点击会同步光标/高亮，切走或隐藏 diff panel 会清理主视图投影
- [x] 主 hex view / diff panel 顶部已固定 byte 列头（默认 `00 01 02 ... 0F`），滚动时不消失；鼠标命中只计算列头下方的数据行
- [x] Tab 已固定为 side panel 可见性开关：panel 已显示时直接关闭并回到 Normal，不再先切入 panel 焦点
- [x] release 已切到 `OS * arch * feature` 产物矩阵（`core` / `default` / `full`）
- [x] 仓库自身源码当前以 `MIT OR Apache-2.0` 双许可发布；`full` 产物继续附带 `licenses/keystone/FOSS-NOTICE.txt` 与 Keystone 的 license / exception 文件，不再把 `full` 描述成 `MIT/Apache-only`
- [x] App 超大文件首轮拆分已完成：`render`、`events`、`commands` 入口保留轻量分发，领域实现移到对应子模块；本轮为 move-only 结构整理，不改变编辑语义
- [x] `memory` feature 下已支持 `:mem freeze` / `:mem thaw` 对目标进程做可嵌套暂停/恢复，commit 运行中目标时会给出安全提示
- [x] `memory` 模式下 `:w` 等价 `:mem commit`，`:w <path>` 被拒绝并提示改用 `:export`；`MemorySession` 现按 region 持久化未提交 replacement / undo / redo（切换 region 不再丢失编辑），`:mem commit-all` 真正按 VA 升序遍历所有 dirty region 并在首个失败处停下保留 dirty；`:q` 在任意 region dirty 时拒绝并汇总 `N regions dirty, total M bytes`，`:q!` 丢弃
- [x] `:mem info` 已聚合：选中 region + 指纹、选中 region 的 dirty 字节与 undo/redo 深度、session 级 dirty region 数与总字节、dirty region 列表（含 stale-base 标记）、backend ro/rw、freeze 状态
- [x] 跨 region 字节搜索改用 `memchr::memmem`（`find` / `rfind`，保留分块 + overlap）；`:ms` / `:ms!` 命中后记忆 query，`gn` / `gN` 重放（独立于文件搜索 `n` / `p`，不复用 `last_search`）
- [x] memory side panel 引入 Maps / ProcessList / Info 三视图：`:mem list` 进程列表占满面板（Enter attach，含 dirty 守卫）、`:mem info` 多行报告占满面板；全部支持上下选择 + 鼠标滚轮滚动；Maps 行区分 selected(光标 `▶`) 与 opened(已加载 `●`) 高亮，不可读/ stale 行着色；鼠标点击 region/进程行只切高亮

---

## Core / Correctness（已完成）

- [x] **[P1] `:hash` 对大文件缺少进度反馈**
  - 已解决：TUI `:hash` 对超过分段阈值的大范围先显示 `hashing...` 状态，再按分段继续流式处理 logical bytes，状态栏显示 checked / logical hashed 进度，扫描中阻塞其它输入并支持 Esc 取消
  - 保持：小范围 hash、headless `--command`、macro / script 仍使用同步执行层；底层仍走 `Document::walk_logical_chunks` / 64 KB logical chunk，不整文件物化
  - 已同步：App 回归测试、命令 hint、用户指南与当前 backlog

- [x] **[P1] `:export` / `:fill` / `:xor!` 在大选区 / 大长度下会一次性物化整段 bytes**
  - 已解决：三条路径改为 streaming，单次 buffer 与 `:hash` 一致按 64 KB chunk（并按 `page_size * cache_pages` 上限裁剪，避免 `read_range` 跨页失效）
    - `:export <path>` binary：经 `Document::for_each_logical_chunk` 边读 chunk 边写 `BufWriter`，不再 `logical_bytes` 整段物化
    - `:fill` / `:zero`：经 `Document::overwrite_run_positional` 按 chunk 解析 cell 并写 replacement，不再生成完整 pattern buffer
    - `:xor!`：经 `Document::transform_visible_range_in_place` 按 chunk 读 → xor → 写回 replacement，不再整段读出
  - 范围之外：C array / Python bytes 导出仍走完整文本（受剪贴板上限约束，影响较小）；`:xor`（复制）仍生成完整 hex 文本到剪贴板
  - 已同步：`benches/perf_bench.rs` 增 export/fill/xor! streaming bench；`src/app/tests.rs` 增跨 64 KB chunk 回归测试；status 文案保持 logical bytes / display span 对照

---

## UX / Editor Features（已完成）

- [x] **[P2] 搜索命令面：统一 `:s [mode]<delim><text><delim>`，废弃 `:S`**
  - 现状：`:s <text>` 走 ASCII，`:S <hex>` 走 hex，`!` 后缀切方向；模式靠不同命令字母区分，扩展性差（typed integer、wide string 等都没地方放）
  - 目标：把 mode 做成 `:s` 的前缀参数，统一搜索入口
    - 语法：`:s [mode]<delim><text><delim> [filter...]`；mode 之后第一个非字母数字字符即分隔符（vim 风格），允许 `/` `,` `#` `@` 等替代以避开 pattern 内的 `/`
    - mode 列表（v1 落地）：
      - 无 / `/foo/` — UTF-8 字符串（默认）
      - `x/4889c7/` — 原始 hex 字节，允许空格分隔（`x/48 89 c7/`）
      - `b/255/` — 单字节十进制
      - `u32/12345/` `u32le/...` `u32be/...` — u32 整数，无后缀走当前文档 / 进程 arch 默认端序
      - `u64/...` `u64le/...` `u64be/...` — u64 整数
      - `i32/...` `i64/...` — 带符号变体
    - mode 列表（后续按需扩展）：`f32` / `f64` 浮点、`w/foo/` UTF-16 LE wide string、`r/regex/` 正则
    - `!` 后缀继续表示反向：`:s! x/cafe/`
  - 兼容性：`:S` 在过渡版本作为 `:s x/.../` 的 deprecated 别名保留，命中时状态栏提示；下一版彻底移除 `:S`
  - 已同步：README / README_CN / command hints / search 测试用例 / `AGENTS.md` 提到搜索命令的段落
  - 依赖：跨区内存搜索 `:ms` / `:ms!` 已沿用同一 mode 前缀语法，参见 `mem-design.md` §9.2 / §9.3

---

## Performance / Maintenance（已完成）

- [x] **[P1] App 层回归仍需继续补**
  - 已补齐：新增集中 App 回归覆盖 paste overwrite / insert、visual delete、inspector edit 提交、undo / redo、长度变化后的 mode clamp、command submission 返回 Visual / SidePanel mode

- [x] **[P2] 性能类测试应迁移到 `cargo bench` 管理**
  - 现状：性能观测已迁移到 `cargo bench --bench perf_bench`，覆盖 16MB/64MB save 场景、hash / logical_bytes / paste overwrite / piece lookup / ELF parse / 当前 search 路径
  - 已完成：`cargo test` 中的大文件用例只保留 correctness 断言，不再使用 wall-clock 阈值；benchmark 输出统一由 bench harness 管理

- [x] **[P1] clean range replacement undo 避免 per-byte 展开**
  - 已解决：`ReplacementStore` 支持稳定 `CellId` range overlay；clean `:fill` / `:zero` / `:xor!` 走 compact `EditOp::ReplaceBulk`，undo/redo 通过 clear / pattern / xor bulk record 恢复，不再为大范围 clean overwrite 保存逐字节 undo
  - 已解决：`:fill` / `:zero` 的语义是覆盖范围全部改变，clean fast path 不再额外扫描 base bytes 统计 changed，避免大范围 no-op 检测拖慢 apply
  - 范围之外：已有 replacement/tombstone 的 dirty range 仍保留在当前 backlog，不能用 clean range 语义直接覆盖

- [x] **[P1] 等长 `:re` 避免默认 OOM**
  - 已解决：等长 `:re` 默认先预检至多 65536 个 match，超过 65535 不执行并提示使用 `--force`
  - 已解决：等长 `:re --force` 按批扫描 / 应用，不再一次性收集全量 match；clean 连续命中压成 pattern range overlay + compact undo，tombstone 会切断可匹配 segment
  - 范围之外：变长 `:re!` 仍在当前 backlog，需要单独设计快照搜索和 offset delta 映射

- [x] **[P2] `:diff` next/prev mismatch 避免不可取消同步全量扫描**
  - 已解决：diff mismatch navigation 改为分块扫描 / 可取消 stepper；长距离 mismatch 查找期间只允许 `Esc` 取消，并定期刷新进度，避免 1GB 级文件上长时间无反馈卡住 UI
  - 范围之外：跨页 shift-aware alignment cache / progress 仍保留为后续可选 backlog

- [x] **[P2] 文件搜索 clean / sparse-dirty 路径换成 chunked memmem**
  - 已解决：clean 文档扫描换成 `memchr::memmem::find` / `rfind`，保留 chunk + `pattern.len()-1` overlap；256MB worst-case 从约 307ms 降到约 53ms
  - 已解决：dirty 文档不再整篇退回逐字节 KMP；逐 chunk 判定脏净，clean chunk 走 memmem，只有含 tombstone / replacement 的 chunk 才逐 cell；256MB + 单 tombstone worst-case 从约 333ms 降到约 38ms
  - 范围之外：dense range overlay search 仍可能同步卡顿，保留在当前 backlog
