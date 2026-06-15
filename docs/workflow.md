# Workflow（工作方式、常见改动路径、自检清单）

---

## 1. 当前优先级共识

在没有更高层新指令时，优先做：

1. 修核心 bug
2. 补测试缺口
3. 修语义 / 文档不一致
4. 再做新功能
5. 最后做纯样式类优化

---

## 2. 推荐工作方式

### 小改动

- 直接改
- 补对应测试
- 更新文档

### 大改动

先写清楚：

- 目标语义
- 影响模块
- 是否影响 undo / save / search / cursor
- 需要新增哪些测试

如果做不到这一步，先不要下手大改。

对于 quality / 重构类修改，优先：

- 先做 helper 提取或分发拆分，不要一口气改语义
- 优先删除无主的 `helpers.rs` / 杂项工具桶，把 helper 收回调用方或所属模块后，再决定是否需要共享抽象
- 能按模块拆开时，优先拆成独立文件，避免继续把输入状态机堆回一个文件
- 每个阶段都跑最相关的一组测试
- 最后再跑全量 `cargo fmt --check`、`cargo test --all-targets`、`cargo clippy --all-targets`

---

## 3. 常见改动路径

### 3.1 新增一个 `:` 命令

推荐顺序：

1. `src/commands/types.rs` 增加命令类型
2. `src/commands/parser.rs` 增加解析
3. `src/commands/hints.rs` 增加提示
4. `src/app/commands.rs` 接分发，并把执行逻辑放到 `src/app/commands/*` 对应领域文件
5. 补 parser / app 行为测试
6. 同步 README / `docs/issues.md`（若是新用户功能）

### 3.2 修改编辑语义

先明确你改的是：

- real delete
- tombstone delete
- replacement

每次至少复查：

- display offset 是否应移动
- save 结果是否正确
- undo / redo 是否可逆
- search / selection / cursor clamp 是否受影响

### 3.3 修改 inspector / format

推荐顺序：

1. 先确认 detect 可靠
2. 再确认 parse 输出稳定
3. 决定是否需要分页 / `:insp more`
4. 谨慎开放 editable 字段
5. 补 parse / render / App 回归

特别注意：改 `NodePath`、`flatten()`、`count_skipped_fields()` 时，很容易把折叠态和字段编辑定位搞坏。

### 3.4 修改 save 逻辑

这是高风险路径，至少要验证：

- overwrite-only
- insert-only
- tombstone-only
- replacement-only
- 混合编辑
- readonly / save-as
- save 后 dirty / undo / inspector / cursor 是否一致

### 3.5 为 document fuzz 添加新操作

当前固定 seed fuzz 在 `tests/insert_mode.rs` 的
`deterministic_mixed_document_edit_fuzz_matches_reference_model`。它用
`ReferenceDoc { slots: Vec<Option<u8>> }` 做 byte 级模型：`Some(byte)` 是可见
display slot，`None` 是 tombstone。新增 fuzz 操作时按这个顺序做：

1. 先写清该操作属于 real delete / tombstone delete / replacement / insert 哪一种。
2. 在真实 `Document` 上调用对应 API，同时在 `ReferenceDoc` 上做同构变更。
3. 明确错误语义：例如 overwrite paste 命中 tombstone 应返回错误且不得部分写入；
   这种路径也要在 fuzz 中断言。
4. 每步后继续调用 `ReferenceDoc::assert_matches`，至少比较 `len()`、
   `visible_len()`、`logical_bytes()`、display slot、display/logical offset 映射。
5. 控制规模：固定 seed、小模型、固定步数；不要把 wall-clock 阈值塞进
   `cargo test`。性能观察放到 `benches/perf_bench.rs`。
6. 如果新操作涉及 App undo/redo 栈，只靠 document fuzz 不够；补 App 层连续操作
   测试或专门的 undo/redo bench。

---

## 4. 提交前自检清单

- [ ] 是否区分了 real delete / tombstone / replacement
- [ ] 是否检查了 mode 与 EOF cursor 规则
- [ ] 是否补了测试，且测试真的会执行
- [ ] 是否同步了 README / docs/issues.md / 提示文本
- [ ] 是否没有把 inspector 做成“可编辑但极易产出坏文件”

---

## 5. 一条最重要的原则

**先保护数据模型和交互语义，再追求功能数量。**

`hxedit` 的核心不是“功能越多越好”，而是先守住 byte 级数据模型、undo/save/search 语义和文档一致性，再扩展功能面。
