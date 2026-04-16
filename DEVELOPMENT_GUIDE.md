# hxedit Development Guide

本文档面向后续开发者，目标是把 `hxedit` 当前的实现结构、关键约束、已知风险与推荐开发路径说明清楚，减少“看起来能改、实际上容易把语义改坏”的情况。

---

## 1. 项目目标

`hxedit` 是一个面向**大文件**的 Rust TUI hex editor。当前实现重点不在“全功能文本编辑器式体验”，而在以下几点：

- 大文件可滚动查看
- 以**非破坏性编辑模型**支持 byte 级修改
- 支持 overwrite / insert / tombstone delete / search / save
- 提供格式 inspector（ELF / PNG / ZIP）
- 保持后续继续优化大文件性能的空间

当前代码已经具备可用基础，但仍存在：

- 文档与实现不完全一致
- App 层测试已重新接入，但覆盖仍可继续补强
- Inspector 与格式编辑的一致性保护不足
- 部分热点路径仍是线性扫描

---

## 2. 代码结构总览

### 2.1 顶层模块

- `src/app.rs`
  - 运行时主状态 `App`
  - TUI 事件循环
  - 模块编排入口

- `src/core/`
  - 核心文档模型与底层 IO
  - `document.rs`：Document 抽象、搜索、保存入口
  - `piece_table.rs`：插入/真实删除核心数据结构
  - `file_view.rs` / `page_cache.rs`：分页读取与缓存
  - `save.rs`：保存实现

- `src/input/`
  - 各 mode 的键盘映射

- `src/view/`
  - 渲染布局与 UI 组件

- `src/commands/`
  - `:` 命令解析、类型定义、提示文案

- `src/format/`
  - 格式识别、结构解析、inspector 编辑
  - 当前内置：ELF / PNG / ZIP

- `tests/`
  - 集成测试
  - 当前对 document/search/save 的覆盖较好
  - 对 App 层交互覆盖不足

### 2.2 App 子模块

- `app/events.rs`
  - Action 分发中心
  - 鼠标 / 键盘驱动状态变化

- `app/state.rs`
  - 删除、编辑、插入、selection、inspector 状态逻辑

- `app/navigation.rs`
  - 光标、viewport、EOF 边界规则

- `app/undo.rs`
  - undo 栈回放

- `app/clipboard_ops.rs`
  - copy / paste 用户操作逻辑

- `app/commands.rs`
  - 命令执行入口

- `app/render.rs`
  - 主界面、状态栏、命令栏、inspector 渲染

---

## 3. 核心数据模型：Document 的三层语义

`Document` 不是直接修改原文件字节，而是由三层组合而成。

### 3.1 第一层：PieceTable

`PieceTable` 负责：

- 插入字节
- 真实删除（real delete，主要用于 insert/backspace）

其语义是：

- 原文件内容永不原地改写
- 新插入字节写入 add-buffer
- 文档内容由一组 piece 映射而成

### 3.2 第二层：Tombstones

`tombstones: BTreeSet<CellId>` 负责：

- 正常删除 / visual 删除
- 删除后仍保留 display slot

也就是说：

- tombstone 不会让后续 offset 左移
- UI 里会显示为 `Deleted`
- save 时会跳过这些字节

### 3.3 第三层：Replacements

`replacements: BTreeMap<CellId, u8>` 负责：

- nibble edit
- overwrite byte edit
- inspector 写回字段

替换发生在稳定 `CellId` 上，而不是“当前 display offset 上的那一格”。

---

## 4. 必须保持的关键不变量

后续开发中，这些约束尽量不要破坏。

### 4.1 长度语义

- `original_len()`
  - 原始文件长度

- `len()`
  - 当前 display stream 长度
  - **包含 tombstone 占位**

- `visible_len()`
  - save 时真正会落盘的逻辑长度
  - 近似 `piece_table.len - tombstones`

### 4.2 Offset 语义

- App 层多数操作面对的是 **display offset**
- save / copy / search 的结果如果不是 display offset，必须明确标注
- 不要混淆：
  - display span
  - logical bytes count
  - original file offset

### 4.3 光标语义

- `Normal` / `Visual`
  - 光标不应该停在 EOF 之后

- `EditHex` / `InsertHex`
  - 允许光标位于 `len()`（EOF 追加场景）

若修改 undo / save / search / paste / mode 切换逻辑，必须重新检查 EOF clamp。

### 4.4 Tombstone 语义

- tombstone delete 不改变 display 布局
- real delete 会立刻改变 display 布局
- 两种删除不可混用语义

### 4.5 Replacement 语义

- replacement 是“覆盖显示值”
- 若写回值与 base byte 相同，应移除 replacement，而不是保留冗余状态

### 4.6 Save 语义

当前实现中：

- `Document::save()` 实际统一走 rewrite path
- save 完成后会：
  - reload 文件
  - 重建 piece table
  - 清空 tombstones
  - 清空 replacements

如果未来实现 patch-save，需要确保：

- 与 rewrite-save 的最终文档状态一致
- save 后 dirty 状态、undo 状态、cursor 状态语义保持统一

---

## 5. 当前已知风险与技术债

### 5.1 App 层测试已接回，但覆盖还要继续补

`src/app/tests.rs` 现已通过 `#[cfg(test)]` 接入编译，基础 App 交互不再处于“文件存在但没跑”的状态。

这意味着：

- 当前 `cargo test` 已能覆盖一部分 App 行为回归
- 但仍不等于所有 mode / command / inspector 交互都已充分覆盖

后续若改动：

- mode 切换
- command buffer
- visual search
- paste UX
- inspector interaction

优先在真正执行的单测/集成测试里补回归，不要再新增“看起来像测试、实际上没接入”的文件。

### 5.2 文档和实现有漂移

最明显的是：

- README 写了 patch-in-place save
- 当前实现总是 rewrite save

处理原则：

- 用户可见行为改了，就同步 README
- README 承诺了但还没实现，就在 issues 中显式跟踪

### 5.3 Inspector 是“全量刷新”实现

当前很多编辑路径会：

1. detect format
2. parse format
3. flatten rows
4. rebuild render data

这样逻辑简单，但性能不够理想，且格式定义越复杂越明显。

### 5.4 格式编辑缺少强校验

例如：

- PNG 改 chunk 相关字段时不会自动修 CRC
- ZIP 改 header 字段时不会做一致性修复

因此“字段可编辑”不等于“结构编辑安全”。

### 5.5 非致命失败必须可观测

当前约束：

- inspector 解析失败时，要在 panel / 状态消息中明确区分“没识别到格式”和“识别到了但解析失败”
- render 读数据失败时，不要直接静默 fallback；至少要留 stderr 或状态提示
- 新代码尽量避免用 `unwrap_or_default()` 掩盖 parse / render / refresh 失败

### 5.6 大函数优先做“分发 + helper”式拆分

当前已经完成一轮基础拆分：

- `handle_action` 改为按 navigation / command / inspector / editor 分发
- `execute_command` 改为按命令类别分发到私有 handler
- `render_main` 改为按取数、组装 line、主面板绘制、inspector 绘制分段

后续继续重构时，优先沿着这个方向扩展，而不是把分支重新堆回单个超大函数。

---

## 6. 当前最值得优先处理的方向

### P0

1. 明确 overwrite paste 的 EOF 语义，并统一 README / 实现 / 测试
2. 解决窄屏 inspector 焦点不可见问题
3. 修 ZIP data descriptor 场景
4. 继续补 App 层交互回归，尤其 paste / inspector / undo command

### P1

1. 实现真实 patch-save fast path，或调整文档承诺
2. 处理 Normal 模式 EOF cursor 边界
3. 优化 `PieceTable::resolve()` 与大选区批处理
4. Inspector 增量刷新

### P2

1. 命令历史、redo、wrap search
2. readonly 自动降级
3. 更强状态栏信息与 paste preview

---

## 7. 常见开发任务的推荐路径

### 7.1 新增一个 `:` 命令

建议顺序：

1. 在 `src/commands/types.rs` 增加 `Command`
2. 在 `src/commands/parser.rs` 增加解析
3. 在 `src/commands/hints.rs` 补提示
4. 在 `src/app/commands.rs` 执行命令
5. 补 parser 测试、行为测试、README

### 7.2 修改搜索行为

涉及模块：

- `app/search.rs`
- `app/events.rs`
- 可能还包括 `state.rs` / `navigation.rs`

必须检查：

- Visual 模式如何处理选区
- 命中后 cursor 是否应移动
- backward 搜索边界是否合理
- profiler 记录是否仍成立

### 7.2.1 修改事件或命令分发逻辑

建议顺序：

1. 先判断改动属于 navigation / command / inspector / editor 哪一层
2. 优先改对应私有 helper，不要直接把逻辑重新塞回总入口
3. 每个阶段先跑相关子集测试
4. 最后再跑全量 `fmt / test / clippy`

### 7.3 修改删除/插入语义

首先先判断你改的是哪一种：

- real delete
- tombstone delete
- replacement edit

不要把三者混成同一种“改字节”。

每次修改都建议验证：

- offset 是否移动
- search 是否受影响
- save 是否正确
- undo 是否能完全恢复

### 7.4 新增一个格式定义

建议顺序：

1. 在 `src/format/defs/` 新增定义文件
2. 补 detect
3. 只先做**稳定且可验证**的字段
4. 明确每个字段是否 `editable`
5. 补 fixtures 与 parse/edit 测试

原则：

- 不要把“理论上能写”的字段都暴露为 editable
- 对结构一致性要求高的字段，宁可先只读

### 7.5 改 save 逻辑

这是高风险改动。

至少要验证：

- overwrite-only
- insert only
- tombstone only
- replacement only
- insert + tombstone 混合
- save as 新路径
- readonly 行为
- save 后 dirty / undo / inspector 是否一致

---

## 8. 测试建议

### 8.1 当前推荐命令

至少运行：

```bash
cargo test --all-targets
```

如果改了较多代码，再运行：

```bash
cargo clippy --all-targets
```

注意：当前仓库并不是严格 `-D warnings` clean，直接跑超严格 clippy 会被现存 lint debt 卡住。

### 8.2 测试分层建议

- **`tests/`**
  - 适合 document/save/search 的真实回归

- **模块内单测**
  - 适合 parser / view / helper / format edit

- **App 交互测试**
  - 当前是薄弱区，建议补键盘动作驱动的状态机测试

### 8.3 对高风险改动的最低测试要求

若修改以下模块，至少补一条回归测试：

- `clipboard_ops.rs`
- `undo.rs`
- `navigation.rs`
- `state.rs`
- `format/edit.rs`
- `core/save.rs`
- `core/document.rs`

---

## 9. 文档同步约定

用户可见行为发生变化时，需要同时检查：

- `README.md`
- `issues.md`
- 相关命令提示（`commands/hints.rs`）
- 测试名与注释

不要出现：

- README 还写着旧语义
- 测试覆盖旧 API
- issue 已过时但仍保留

---

## 10. 建议的“完成定义”

一个较大的改动，至少满足：

1. 实现语义和 README 一致
2. 新旧路径都有回归测试
3. undo / save / search 没有被顺手破坏
4. mode / cursor / viewport 没有进入非法状态
5. 如改动用户交互，状态栏反馈合理

---

## 11. 一句话总结

`hxedit` 当前最重要的不是“疯狂加功能”，而是：

**先把核心编辑语义、测试覆盖、文档一致性和热点路径做扎实，再继续往 inspector 和高级交互上扩。**
