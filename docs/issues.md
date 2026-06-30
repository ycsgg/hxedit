# hxedit Issues

本文件只保留**当前仍需要行动**的事项。
已完成或已过期的整理项见 `docs/archive/completed.md`，并交给 git history、测试和提交记录，不要继续在这里堆长列表。

---

## UX / Editor Features

- [ ] **[P2] Remote 文件编辑后续增强**
  - 设计：见 `docs/remote-editing-design.md`
  - 现状：默认已具备 `--remote sftp://...` / `ssh://...`（`remote-sftp` feature）、可选具备 `ftp://...`（`remote-ftp` feature）、`russh-sftp` transport、passive binary FTP、page-cache 分块读取、clean remote hash/search/export streaming fast path、fake remote 测试后端、远程 rewrite-save、fingerprint 冲突检测；remote original bytes 仍走现有 `Document` piece table / tombstone / replacement 模型
  - 后续：remote other side for `:diff`、长操作进度、remote save-as 明确语义、更多协议 capability 分层与集成测试矩阵

- [ ] **[P1] Inspector 缺少跨 struct 快速跳转**
  - 建议：支持 `{` / `}` 或 `Shift-J / Shift-K` 跳到上一个 / 下一个 Header

- [ ] **[P2] Inspector 缺少全量展开 / 折叠**
  - 建议：支持 `zR` / `zM`，或 `:insp expand` / `:insp collapse`

- [ ] **[P2] 配置文件后续扩展项**
  - 现状：已落地 TOML 配置文件（`[display]` / `[behavior]` / `[performance]`），覆盖 bytes_per_line / data_panel_bytes / inspector_depth / export_* / readonly / inspector / color(auto\|never) / search_wrap / page_size / cache_pages
  - 待评估（按需）：`color` 显式多级（basic/256/truecolor）、`:hash` 默认算法、data panel typed-value 端序（需贯穿改造，风险高、单独立项）、diff `DiffOptions` 暴露、IO chunk 调参

- [ ] **[P2] 命令历史尚未持久化**
  - 建议：按 XDG 路径持久化，限制条数，避免无限增长

- [ ] **[P2] 书签 / mark 仍缺失**
  - 目标：支持类似 vim 的 `ma` / `'a`，方便在大文件中往返跳转

- [ ] **[P2] 原始 binary clipboard 写仍未落地**
  - 现状：copy 以文本表示为主；base64 已可用，但仍不等于真实 raw bytes clipboard

- [ ] **[P3] 运行时修改 bytes-per-line**
  - 现状：已支持配置文件 `[display].bytes_per_line` 与 `--bytes-per-line`，但仍是启动期生效
  - 目标：支持 `:set bpl <n>`，无需退出重开

- [ ] **[P3] 基于 inspector 字段的跳转 / follow pointer**
  - 例如：`:g field e_phoff`、`:g field sh_offset`
  - 依赖更完整的结构化视图

- [ ] **[P3] 结构化搜索**
  - 例如：按字段值而不是纯 byte 序列搜索；更适合在格式支持做深之后推进

---

## Format Expansion Roadmap

### 更多内置格式支持

- [ ] **[P1] 继续补齐现有格式深度**
  - 当前格式覆盖面已够宽，近期重点不再是“再加一个格式名”
  - 应优先补 PE、Mach-O 等现有格式的深层结构、跳转与更稳定的编辑边界；ELF 已具备 section / symbol / relocation / dynamic / note / hash / version 等结构和分页，后续重点转为结构间跳转、联动与保守编辑；ZIP 已具备 central directory / EOCD / ZIP64 / data descriptor 感知，SQLite 已具备 database header / b-tree page 轻量解析，PCAP/PCAPNG 已具备 capture/block/packet data range 轻量解析；classic PCAP timestamp、GZIP/PE timestamp、ZIP DOS modified time、TAR octal mode/size/mtime、GIF frame delay 已支持可读合成字段双向编辑，后续重点是结构间跳转与一致性提示

### ELF 后续计划

- [ ] **[P2] 结构间跳转与联动**
  - 从 `e_phoff` / `e_shoff` / `p_offset` / `sh_offset` 直接跳到对应表项或数据范围
  - 让 ELF inspector 不只是“列表展示”，还能快速导航

- [ ] **[P3] 保守开放深层表项编辑**
  - 默认先把深层表项做成 view-only
  - 只为低风险字段开放编辑，继续避免把“可编辑”误导成“结构安全”

---

## Disassembly Follow-up Roadmap

详细实施拆分与建议 PR 顺序维护在 `disassemble-design.md`，这里仅保留 backlog 级目标。

- [ ] **[P1] Keystone backend 仍偏老旧且维护性不足**
  - 现状：当前 `full` 已切到 `hexpatch-keystone` 路线，但整体 Keystone 生态仍偏老、上游活跃度有限，后续 toolchain / crates.io / 跨平台构建风险仍在
  - 目标：评估并逐步迁移到更现代、可维护的 assembler backend；优先关注 `iced-x86`、`zydis`、`asmjit` 等方案；若只需覆盖少量简单指令集，也可评估为特定架构直接维护最小自研 patch backend

- [ ] **[P1] patch 后缓存失效与局部重解码继续细化**
  - 当前已有 row cache / checkpoint，但更复杂 patch 路径仍要继续验证

- [ ] **[P1] disassembly 下的 undo / save / search 回归继续加深**
  - 重点覆盖 overwrite patch、symbol search、mode clamp、side panel 交互

- [ ] **[P2] raw disassembly 强制模式继续扩展 arch / endianness 选项**
  - 当前 `:dis!` 仍偏保守，参数面和报错说明还有优化空间

- [ ] **[P2] 汇编 patch 继续做 symbol / expression 解析增强**
  - 当前只覆盖 direct branch/call 的单 token symbol/import 名

- [ ] **[P2] Sagitta snapshot 继续增强 disassembly annotation**
  - 现状：`sagitta-analysis` feature 已能通过 crates.io `sagitta-rs` 后台分析 current logical bytes，并用 recovered functions 覆盖 symbol panel、函数入口 label、direct branch target name、函数体 rail 和 `:symbol` 搜索
  - 缺口：后续按 `docs/sagitta-integration-design.md` §13 接入 resolved indirect jump、switch/jump-table、tail call / callgraph 与 CFG annotation；当前 jump rail 仍只使用 Capstone direct target

- [ ] **[P2] 首批架构之外的 decoder 支持**
  - 后续再评估 `x86`、`arm`、`riscv64` 与替代 backend

---

## Performance / Maintenance

- [ ] **[P1] dirty range replacement 仍需避免 per-byte 展开**
  - 现状：clean range overlay 已落地并归档；但已有 replacement/tombstone 的 dirty range 仍退回 per-byte undo / apply
  - 建议：继续扩展 `ReplacementStore` 的分段 overlay 能力，覆盖 dirty range 的批量 `SetBytes/SetPattern` 场景；dirty range 需要能正确保留已有 sparse override / clear hole，不能为了省内存破坏 replacement-only 语义

- [ ] **[P1] memory replacement spans 仍会展开 range overlay**
  - 现状：文件 save / export 走 walker 流式消费 overlay，RSS 保持低；但 memory 模式的 `replacement_spans()` 会把 range overlay 展开成 `Vec<(offset, Vec<u8>)>`，用于 `:mem commit`、`:mem commit-all` 与 region switch stash。当前 256MB overlay span 峰值约 275MB RSS，1GB region 在 1C1G 环境有 OOM 风险
  - 归属：交给 memory 分支处理；当前主线只保留风险记录
  - 建议：active region commit 先改为 streaming span visitor，避免提交时一次性物化；region switch / stashed region 需要单独设计 snapshot 语义，不能简单保存 XOR/pattern overlay 后再基于之后的 live bytes 重算

- [ ] **[P1] `:re!` 仍需要批量 match job，避免一次性收集全量匹配**
  - 现状：等长 `:re` / 等长 `:re!` 的 OOM 止血已落地并走 replacement-only streaming 路径；真正变长的 `:re!` 默认也会先用 65535 match 上限要求 `--force` 确认；但 `--force` 后仍会先收集全部 match offset，低熵大文件上小 needle 仍可能产生海量 match，提前 OOM，且无进度/取消
  - 建议：真正变长的 `:re! --force` 需要基于替换前快照搜索并用 delta 映射到 live document，避免新插入内容影响后续匹配语义；实现形态应是 job/stepper 或受预算的批处理，带进度与取消

- [ ] **[P2] 格式检测 / Inspector 单字段扫描缺少预算**
  - 现状：entry cap 只限制结构数量；GZIP FNAME/FCOMMENT、JPEG scan data、GIF sub-block 等单字段仍可能在畸形大文件中同步扫到 EOF，导致 UI 长时间无响应
  - 建议：为格式解析增加统一 scan budget；超预算时保留已解析结构并标记 truncated / budget-exceeded，不继续依赖不可信 cursor 解析后续字段

- [ ] **[P2] `:diff` 后台 alignment cache / progress 仍未落地**
  - 现状：`:diff` 打开和 next/prev mismatch 的不可取消全量同步扫描已落地修复并归档；可见页仍只在 `max_shift` 范围内做局部重对齐并处理投影 cell 的点击/清理
  - 目标：若后续需要跨页 shift-aware 状态，应做成可取消的后台/stepper alignment cache，不能阻塞 `:diff` 打开，也不能退回 hunk 列表主体验

- [ ] **[P2] dense range overlay 搜索仍可能同步卡顿（按需）**
  - 现状：clean / sparse-dirty 搜索优化已落地并归档；但 dense range overlay 会让每个 chunk 都走 dirty replacement 计算，当前 1GB overlay miss 约 4.7s、RSS 约 6.5MB
  - 建议：按真实诉求推进后台 job（`std::thread + mpsc + Arc<AtomicBool>`、进度、`Esc` 取消）或 range-aware search（例如 XOR overlay 把 needle 反变换后走 memmem）

- [ ] **[P2] crates.io 发布仍缺少 CI/CD 自动化**
  - 现状：当前已可本地 `cargo package` / `cargo publish --dry-run`，但真正的 crates.io 发布仍依赖手工操作
  - 目标：补齐 tag / release 驱动的 crates.io 自动化发布流程，至少包含 dry-run、版本/tag 一致性检查、发布前校验与失败可观测日志

- [ ] **[P1] Inspector 仍是全量 refresh**
  - 目前可接受；若未来格式树更深、表项更多，再评估增量刷新

- [ ] **[P2] 继续收敛 App 层模块边界**
  - 首轮已拆出 `src/app/render/*`、`src/app/events/*`、`src/app/commands/*`；后续只做小步 move-only / helper 提取，避免在结构整理中夹带语义改动
  - 优先继续压小 diff projection / transform 等仍偏大的领域文件，并保持相关 App 回归测试可执行

- [ ] **[P2] 继续保持文档短而当前**
  - 每次命令面或格式支持变化时，同步 README / AGENTS / docs/ / docs/issues.md / hints / tests

---

## Deferred / Not Now

- [ ] **memory 模式后续项（见 `mem-design.md`）**
  - [ ] 跨 region instruction-text 搜索 `:mi` / `:mI`（§9.5）
  - [ ] 跨 region 字节搜索改后台 job / 进度 / cancel（§10）
  - [ ] macOS Mach backend（§3，目前 Linux procfs + Windows ToolHelp/VirtualQueryEx 已真实现；macOS 仍返回 unavailable，需 `task_for_pid` + `mach_vm_*`，受 root / debugger entitlement 限制）
  - [ ] memory panel region-level diff（region 内 original vs 当前逐字节着色，§4.2）

- [ ] **overwrite-only patch-save fast path**
  - 当前 rewrite-save 成本尚可，且实现会深度耦合 replacement / tombstone / real delete 语义
  - 没有明确性能证据前，先不作为近期主线

- [ ] **更激进的 inspector 增量刷新**
  - 在当前 parse 成本下收益有限；先把格式深度和正确性做扎实
