# hxedit Issues

本文件只保留**当前仍需要行动**的事项。
已完成或已过期的整理项见 `docs/archive/completed.md`，并交给 git history、测试和提交记录，不要继续在这里堆长列表。

---

## Core / Correctness

- [ ] **[P1] `:hash` 对大文件缺少进度反馈**
  - 现状：64 KB 分块已就位，但 GB 级文件仍会让 UI 长时间静默
  - 建议：开始先显示 `hashing...`，之后每 N 个 chunk 刷一次百分比或已处理字节数

- [ ] **[P1] ELF inspector 仍只有 Header + Program Header Table**
  - 现状：Section Header Table、符号表、重定位表、动态段等都还没展开
  - 额外问题：ELF 当前也还没有像 PNG / ZIP 一样接入 `:insp more` 分页路径

- [ ] **[P2] magic / header 被编辑后，inspector 的“格式丢失”反馈仍不够明确**
  - 现状：格式从可识别变为不可识别时，用户容易只看到 inspector 消失
  - 建议：从 Some → None 时给出显式状态提示，如 `format lost: header modified`

---

## UX / Editor Features

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
  - 应优先补 ELF、PE、Mach-O 等现有格式的深层结构、分页、跳转与更稳定的编辑边界；ZIP 已具备 central directory / EOCD / ZIP64 / data descriptor 感知，后续重点是结构间跳转与一致性提示

### ELF 解析扩展计划

- [ ] **[P1] Phase 1：Section Header Table + section name string table**
  - 显示 `e_shoff / e_shnum / e_shstrndx` 对应的 section 列表
  - 为每个 section 提供 `section_data` 范围字段
  - 大表格要接入分页能力，不要一次性塞满 inspector

- [ ] **[P1] Phase 2：动态相关结构**
  - 覆盖 `PT_DYNAMIC` / `.dynamic`、`PT_INTERP`、常见 note / GNU property 相关段
  - 重点是把“段存在”升级为“段内部字段可浏览”

- [ ] **[P2] Phase 3：符号表与字符串表**
  - 支持 `.symtab` / `.dynsym` / `.strtab` / `.dynstr`
  - 补常见枚举显示，如 symbol bind / type / visibility

- [ ] **[P2] Phase 4：重定位与版本信息**
  - 支持 `REL` / `RELA`、GNU / SysV hash、version needs / defs 等常见结构
  - 目标是让动态链接相关问题能在 inspector 里直接追踪

- [ ] **[P2] Phase 5：结构间跳转与联动**
  - 从 `e_phoff` / `e_shoff` / `p_offset` / `sh_offset` 直接跳到对应表项或数据范围
  - 让 ELF inspector 不只是“列表展示”，还能快速导航

- [ ] **[P3] Phase 6：保守开放编辑**
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

- [ ] **[P2] `:diff` 后台 alignment cache / progress 仍未落地**
  - 现状：UI 已改为同步滚动可见页，打开不再全文件扫描；可见页会在 `max_shift` 范围内做局部重对齐并处理投影 cell 的点击/清理
  - 目标：若后续需要跨页 shift-aware 状态，应做成可取消的后台/stepper alignment cache，不能阻塞 `:diff` 打开，也不能退回 hunk 列表主体验

- [ ] **[P2] `:s` 大文件搜索后台 job + 进度/取消（阶段二，待办，按需）**
  - 已完成（阶段一）：clean 文档扫描换成 `memchr::memmem::find` / `rfind`（`search_clean_forward` / `_backward`），按 chunk + `pattern.len()-1` overlap 续接。256MB worst-case 从 ~307ms 降到 ~53ms，卡顿拐点从 ~256MB 推到 ~2GB
  - 已完成（选项 B）：dirty 文档不再整篇退回逐字节 KMP——`walk_visible_cells` / `_reverse` 逐 chunk 判定脏净，clean chunk 走 `scan_clean_chunk_forward` / `_backward`（memmem + 首尾 `P-1` 字节 KMP 衔接），只有含 tombstone / replacement 的 chunk 才逐 cell。256MB + 单 tombstone worst-case 从 ~333ms 降到 ~38ms（~8×），消除"GB 文件改几字节就退回 KMP"的退化
  - 阶段二（降级为按需）：评测显示换 memmem 后 I/O 成主导（1GB ~120ms 热缓存），主流场景已无明显卡顿；完整后台 job（`std::thread + mpsc + Arc<AtomicBool>`、进度、`Esc` 取消）只为多 GB / 冷盘 / 编辑极密集场景，待出现真实诉求再做
  - 同步项（阶段二）：搜索测试补 cancel / progress 覆盖；hint / README 说明搜索可中断

- [ ] **[P2] crates.io 发布仍缺少 CI/CD 自动化**
  - 现状：当前已可本地 `cargo package` / `cargo publish --dry-run`，但真正的 crates.io 发布仍依赖手工操作
  - 目标：补齐 tag / release 驱动的 crates.io 自动化发布流程，至少包含 dry-run、版本/tag 一致性检查、发布前校验与失败可观测日志

- [ ] **[P1] App 层回归仍需继续补**
  - 优先：paste、visual delete、inspector edit、undo / redo、mode clamp、command return mode

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
