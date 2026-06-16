# Sagitta Integration Design

本文档定义 hxedit 通过 crates.io 上的 `sagitta-rs` 接入 Sagitta 的设计边界。目标是
获得 Sagitta 的符号恢复、函数体识别、CFG/callgraph 与间接跳转解析能力，同时不破坏
hxedit 的 byte 级编辑模型。

---

## 1. 目标

- `:ana` 显式触发 Sagitta 分析当前文档的 logical bytes。
- 分析完成后自动打开 Symbol side panel，但不抢输入焦点。
- Sagitta 分析结果覆盖 symbol panel 的数据源；Sagitta ready 后不再混排 hxedit
  自带 object/dynamic/export symbol。
- `:sym` / `:symbol` / disassembly row annotation 优先使用 Sagitta snapshot。
- 等长编辑后继续允许按原 offset 跳转，但明确提示 analysis outdated。
- 长度变化编辑后强制让 Sagitta 结果失效，要求用户重新 `:ana`。

非目标：

- 不把 Sagitta 结果写入 `Document`。
- 不让 Sagitta 参与 undo / save / search 的核心 byte 语义。
- 不自动在每次编辑后重新分析。
- 不替换现有 Capstone disassembly backend；Sagitta 短期只提供分析/标注。
- 不支持超过 128 MiB logical bytes 的输入。

---

## 2. Feature 与依赖

新增可选 feature：

```toml
sagitta-analysis = ["symbols", "disasm", "dep:sagitta-rs"]
sagitta-rs = { version = "0.0.2", optional = true }
```

依赖必须来自 crates.io，不使用仓库外相对 `path` dependency。进入实现或版本升级前
必须确认：

- `sagitta-rs` 版本与 hxedit 需要的分析 API 兼容。
- `licenses/THIRD_PARTY_NOTICES.txt` 是否需要加入 Sagitta 与其依赖说明。

Sagitta 当前能力边界：

- 支持 x86/x64 ELF/PE。
- 不支持 Mach-O、AArch64、raw forced disassembly。
- `:ana` 遇到 unsupported arch / format 时直接报错 `unsupported arch` 或
  `unsupported format`，不覆盖已有 symbol panel。

---

## 3. 输入语义

Sagitta 必须分析当前文档保存后会落盘的 logical byte stream。

使用：

```rust
Document::for_each_logical_chunk(0, doc.len() - 1, |chunk| ...)
```

不要使用：

```rust
Document::read_logical_range(...)
```

原因：`read_logical_range` 在 tombstone 上会填 `0x00`，更接近 render/format
读取；Sagitta 需要 tombstone 被跳过后的 logical bytes，与 save/export/hash 语义一致。

分析前检查：

```text
document.visible_len() <= 128 MiB
```

超过限制时报错：

```text
Sagitta analysis input too large; limit is 128 MiB
```

这里用 `visible_len()`，因为 Sagitta 分析的是 logical bytes，不是 display len。

---

## 4. Thread Model

Sagitta 在后台线程运行。抽象保持最小，只覆盖当前需要的后台分析结果。

```rust
enum BackgroundJobResult {
    SagittaAnalysis {
        job_id: u64,
        revision: u64,
        result: Result<SagittaSnapshot, String>,
    },
}
```

`App` 保存：

```rust
analysis_job_id: u64,
analysis_state: Option<SagittaAnalysisState>,
background_tx: std::sync::mpsc::Sender<BackgroundJobResult>,
background_rx: std::sync::mpsc::Receiver<BackgroundJobResult>,
```

主循环每轮 input/render 前后调用：

```rust
self.drain_background_results();
```

worker 线程只持有：

- 已物化的 logical bytes。
- `job_id`。
- `document_revision`。
- `Sender<BackgroundJobResult>`。

worker 不持有 `Document`、`App`、`Terminal`、`ExecutableInfo` 或 render state 引用。

Sagitta 内部 invariant 破坏可能 panic。worker 应使用 `std::panic::catch_unwind`，把 panic
转为 `Err(String)` 传回主线程，避免直接杀掉 TUI。

---

## 5. Running State

新增状态：

```rust
enum SagittaStatus {
    Idle,
    Running,
    Ready,
    Failed(String),
}

enum AnalysisValidity {
    Current,
    OutdatedBytes,
    InvalidLayout,
}

struct SagittaAnalysisState {
    status: SagittaStatus,
    validity: AnalysisValidity,
    revision: u64,
    snapshot: Option<SagittaSnapshot>,
}
```

`AnalysisValidity` 语义：

- `Current`：分析结果与当前 bytes 一致。
- `OutdatedBytes`：发生等长 byte 修改，offset 布局仍可用，但分析内容可能过期。
- `InvalidLayout`：发生 logical stream 长度或布局变化，旧 offset 不再可信。

UI 文案避免难懂术语，统一使用：

```text
analysis outdated; rerun :ana
analysis offsets changed; rerun :ana
```

---

## 6. `:ana` Command

命令：

```text
:ana
:ana status
:ana off
```

初始版本只需要 `:ana`、`:ana status`、`:ana off`。后续如果需要暴露深度，再扩展：

```text
:ana functions
:ana indirects
```

行为：

- `:ana` 默认跑 Sagitta `AnalysisDepth::Indirects`。
- `:ana` 运行中再次输入 `:ana`，直接静默丢弃，不排队、不更新状态栏、不创建新线程。
- `:ana status` 显示 running / ready / failed / outdated / invalid layout 与函数数量。
- `:ana off` 清掉 Sagitta snapshot；如果 symbol panel 当前由 Sagitta 提供数据，则关闭或回退到原始 symbol panel，具体按当时 UX 再定。

---

## 7. Result Delivery

主线程收到 `BackgroundJobResult::SagittaAnalysis` 后：

1. 如果 `job_id != self.analysis_job_id`，丢弃。
2. 如果 `result` 是 error，设置 `SagittaStatus::Failed`，不覆盖 symbol panel。
3. 如果 success，安装 snapshot。
4. 如果 `revision == self.document_revision`，validity = `Current`。
5. 如果 revision 不一致，不做二次分析；根据期间编辑记录决定 `OutdatedBytes` 或
   `InvalidLayout`。
6. 用 Sagitta snapshot 重建 `SymbolState`。
7. 自动打开 Symbol side panel，但不抢输入焦点。

不抢焦点规则：

- 如果 symbol panel 已打开：刷新 entries，保持当前 mode/focus。
- 如果 side panel 没打开：打开 side panel 并切到 Symbol，但不把 mode 改成 `SidePanel`。
- 如果用户正在 `Command`、`EditHex`、`InsertHex`、inspector edit 或 disasm edit，只更新数据。
- 如果当前本来就在 `SidePanel`，且 active panel 是 Symbol，保持 side panel mode。

---

## 8. Snapshot Shape

不要把 Sagitta `Analysis` 直接放进 render 或 side panel。转成 hxedit owned snapshot：

```rust
struct SagittaSnapshot {
    summary: SagittaSummary,
    functions: Vec<RecoveredFunction>,
    blocks: Vec<RecoveredBlock>,
    cfg_edges: Vec<RecoveredCfgEdge>,
    call_edges: Vec<RecoveredCallEdge>,
    diagnostics: Vec<RecoveredDiagnostic>,
}

struct RecoveredFunction {
    entry_va: u64,
    entry_logical_offset: Option<u64>,
    name: String,
    name_kind: SymbolNameKind,
    confidence: RecoveredConfidence,
    provenance: Vec<RecoveredFunctionSource>,
    blocks: Vec<u64>,
    callers: Vec<RecoveredCallRef>,
    callees: Vec<RecoveredCallRef>,
}

enum SymbolNameKind {
    Real,
    Synthetic,
}
```

命名规则：

- Sagitta `FunctionView::name() == Some(name)`：使用原名，`SymbolNameKind::Real`。
- `None`：显示 `sub_<addr>`，例如 `sub_401000`，`SymbolNameKind::Synthetic`。
- synthetic name 用暗一点的颜色显示，建议先复用 `palette.disasm_virtual`，只有不够清晰时再加新 palette 字段。

---

## 9. Symbol Panel

`SymbolState` 应从“持有 `ExecutableInfo`”调整为“持有已排序 entries”：

```rust
struct SymbolState {
    entries: Vec<SymbolPanelEntry>,
    source: SymbolPanelSource,
    scroll_offset: usize,
    selected_row: usize,
    detail_scroll_offset: usize,
}

enum SymbolPanelSource {
    Native,
    Sagitta,
}
```

Sagitta ready 后：

- symbol panel 使用 Sagitta entries 覆盖 native entries。
- 不混排 hxedit 自带 symbol。
- `:sym` 在 Sagitta ready 时打开 Sagitta entries；未运行 Sagitta 时保留 native fallback。

entry 至少包含：

```rust
struct SymbolPanelEntry {
    address: u64,
    name: String,
    name_kind: SymbolNameKind,
    size: u64,
    symbol_type: SymbolType,
    source: SymbolPanelEntrySource,
    logical_offset: Option<u64>,
    file_offset: Option<u64>,
    confidence_label: Option<String>,
}
```

`file_offset` 对 UI 来说实际是当前 logical file offset。命名可沿用现有字段以减少改动，
但 detail label 应避免误导：Sagitta entry 的详情显示 `logical`，不是 raw file offset。

---

## 10. Editing Invalidation

不自动重新分析。编辑后只更新 `AnalysisValidity`。

等长编辑标记 `OutdatedBytes`，保留 symbol 跳转与 disassembly annotation：

- hex overwrite。
- nibble edit。
- inspector field write。
- `:fill`。
- `:zero`。
- `:xor!`。
- 默认等长 `:re`。
- disassembly assemble patch。

原因：这些都是 replacement-only 或等长覆盖，不改变 display/logical offset 布局。旧函数入口位置仍可跳，
但 bytes 语义可能已经变了，所以 UI 提示：

```text
analysis outdated; rerun :ana
```

长度或 logical stream 变化标记 `InvalidLayout`：

- insert paste。
- insert mode 插入。
- tombstone delete。
- real delete。
- `:re!` 长度变化。

原因：Sagitta 看到的是 logical stream。tombstone 虽保留 display slot，但 logical stream
会变短，后续 Sagitta logical offset 可能漂移。

`InvalidLayout` 下：

- symbol panel 可保留最后一次列表用于浏览。
- Enter 跳转拒绝：

```text
analysis offsets changed; rerun :ana
```

- disassembly row annotation 和函数体高亮禁用。

---

## 11. Jumping

`Current` / `OutdatedBytes`：

- 按 `entry_logical_offset` 通过 `Document::display_offset_for_logical_offset` 映射到当前 display offset。
- 映射失败时提示：

```text
analysis target is unavailable; rerun :ana
```

`InvalidLayout`：

- 不做跳转。
- 提示：

```text
analysis offsets changed; rerun :ana
```

不引入 `CellId` anchor。按当前约束，等长编辑 offset 不变；长度变化直接让结果失效，比尝试跨布局编辑追踪函数入口更清晰，也更符合“用户在 hex view 做插入/删除通常不是在编辑指令流”的使用模型。

---

## 12. Disassembly Integration

现有 `:dis` 仍使用 Capstone backend 解码。Sagitta 只补充标注：

- 函数入口 label。
- direct target display name。
- 当前 row 所属函数。
- call target name。

启用条件：

- Sagitta snapshot exists。
- validity 是 `Current` 或 `OutdatedBytes`。
- row 的 VA 或 logical offset 能映射到 Sagitta snapshot。

禁用条件：

- validity 是 `InvalidLayout`。
- unsupported arch / format。
- raw forced disassembly。

第一阶段做 symbol label、target name 和函数体 rail；完整函数体高亮 / CFG graph 后续再做。

---

## 13. Future Sagitta Jump / CFG Annotation

当前 disassembly jump rail 的边来自 Capstone 解码得到的 `DisasmRow::direct_target`，不依赖
Sagitta。Sagitta 已经以 `AnalysisDepth::Indirects` 运行，并在 API 中暴露 block-level CFG、
resolved indirect sites、switch/jump-table、callgraph 等更高级的控制流信息；这些能力后续可以
作为 Capstone direct branch rail 的补充，而不是替换现有 disassembly backend。

可接入的数据：

- `Analysis::cfg_edges()`：block-level CFG，`EdgeKind` 可区分 `BranchTaken`、
  `BranchFall`、`Jump`、`IndirectResolved`、`TailCall`、`Call`。
- `Analysis::indirect_sites()`：间接 jump/call 位置、解析状态、resolver kind 与 resolved targets。
- `Analysis::switches()`：jump table dispatch、table、default 与 cases。
- `FunctionView::{callers, callees}` / `Analysis::call_edges()`：direct / indirect / tail call callgraph。
- `BlockView::terminator()` 与 `BlockView::instructions()`：把 block-level edge 映射回具体
  instruction source site 时需要这些信息。

推荐接入顺序：

1. 在 `SagittaSnapshot` 中补 owned 的 indirect-site / switch / block-terminator 视图，保留
   site VA、targets、resolver kind、block start 与 block 最后一条指令 VA。
2. 生成独立的 Sagitta jump edges，只接入 `IndirectResolved` jump 与 switch case/default；
   direct branch/call 继续优先使用 Capstone `direct_target`，同 source/target 去重。
3. 对 `call rax`、tail call、PLT thunk 这类 resolved call edge 先做文本标注，例如
   `[indirect]` / `[tail]` 与 target name；是否画 call rail 另行评估。
4. switch/jump-table target 较多时只画当前 viewport 内可见 targets，屏外 target 用 `⋮` 或
   `+N` 提示，避免 case 过多导致右侧 rail 失控。
5. rail 的 hit test 与 wrap 继续以 disassembly row display source 为准，不能让 Sagitta edge
   overlay 吞掉函数 rail、长指令 wrap 或鼠标定位。

约束：

- `Current` 与 `OutdatedBytes` 可以展示 Sagitta jump/CFG annotation；`OutdatedBytes` 需要复用
  stale 样式或状态提示。
- `InvalidLayout` 禁用 Sagitta jump/CFG annotation，因为 logical offset/VA 对应关系不再可信。
- raw forced disassembly、unsupported arch / format 不使用 Sagitta edge。
- 不把 Sagitta CFG 写回 `Document`，也不让它参与 undo / save / search 语义。

---

## 14. Tests

最低测试覆盖：

- `:ana` running 时重复 `:ana` 静默丢弃，不创建第二个 job。
- `visible_len() > 128 MiB` 时拒绝分析。
- Sagitta success 后自动打开 symbol panel，但不抢 `Command` / edit mode 焦点。
- Sagitta ready 后 `:sym` 使用 Sagitta entries 覆盖 native entries。
- `None` function name 显示为 `sub_<addr>`，并标为 synthetic。
- 等长 replacement 后 validity = `OutdatedBytes`，symbol 跳转仍执行并提示 rerun。
- insert / tombstone delete / real delete / resize replace 后 validity = `InvalidLayout`，symbol 跳转拒绝。
- unsupported arch / format 下 `:ana` 报错，且不覆盖已有 symbol panel。
- worker panic 被转成 failed result，不杀 TUI。

相关文档/提示同步：

- `README.md` / `README_CN.md`
- `docs/user-guide.md` / `docs/user-guide_CN.md`
- `commands/hints.rs`
- `docs/architecture.md`
- `docs/modules.md`
- `docs/issues.md`

---

## 15. Implementation Order

推荐小步实现：

1. 加 feature、命令类型、parser、hint，但 command 先返回 unsupported/disabled。
2. 加 background result channel 与 `drain_background_results`。
3. 实现 logical bytes 物化、128 MiB 限制、worker thread、panic 捕获。
4. 定义 `SagittaSnapshot` owned 转换层。
5. 改 `SymbolState` 为 entries 模型，并保持 native fallback。
6. 接入 Sagitta ready 后自动打开/刷新 Symbol panel，不抢焦点。
7. 接入 editing invalidation：等长 `OutdatedBytes`，长度变化 `InvalidLayout`。
8. 接入 symbol search 与 disassembly annotation。
9. 补测试与文档。
