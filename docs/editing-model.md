# Editing Model & Core Invariants

byte 级数据模型是 hxedit 的核心。改代码前先确定你操作的是哪一类编辑，绝不要“顺手写成另一种”。

---

## 1. 三类编辑（绝对不要混淆）

`Document` 由三层叠加而成：

### A. Real delete / `PieceTable`

- 由 piece table 直接移除内容
- 管真实插入 / 真实删除
- 会让后续 display offset 左移

### B. Tombstone delete / `tombstones: BTreeSet<CellId>`

- 只标记删除，管普通 delete
- 保留 display slot
- save 时跳过

### C. Replacement / `replacements: BTreeMap<CellId, u8>`

- 管 overwrite / nibble edit / inspector field edit
- 只覆盖某个 `CellId` 的显示值
- 不改变 piece 布局
- 大范围 clean replacement 可使用 compact bulk undo op 记录“清除原 replacement / 重放 pattern 或 xor”，但当前实际 replacement 存储仍是 per-cell `BTreeMap`

改代码时先确定你操作的是哪一类，不要“顺手写成另一种”。

---

## 2. 长度与 offset 语义

- `original_len()`：原始文件长度
- `len()`：当前 display 长度，**包含 tombstone 占位**
- `visible_len()`：save 后真正会落盘的逻辑长度

默认情况下：

- App 层导航、光标、Visual selection、`:g` 都是 **display offset** 语义
- copy / export / hash 处理的是**逻辑字节**，如果和 display span 不同，状态栏文案必须说清楚

---

## 3. 光标与 mode 规则

- `Normal` / `Visual`：不应停在 EOF 光标
- `EditHex` / `InsertHex`：可以停在 EOF，用于追加
- Tab 当前是 side panel 可见性开关：panel 隐藏时打开并进入 panel；panel 已显示时关闭并回到 Normal，不作为 Normal 与 panel 的焦点循环键

改以下路径时必须复查 clamp：

- undo / redo
- save 后状态恢复
- search 命中后跳转
- paste / replace / mode switch

---

## 4. Inspector 不变量

- `collapsed_nodes` 现在使用 `NodePath`：`Vec<(struct_name, same-name sibling_index)>`
- `NodePath` 的目标是：结构树前面增删同名节点时，已有折叠态仍尽量落在原来的 struct 上
- `flatten()` / `count_skipped_fields()` 必须继续保证：
  - 折叠分支虽然不展开，但仍要推进 `field_index`
  - `find_field_def` 与行内编辑定位仍然一致
- `refresh_inspector()` 默认保留旧的 `collapsed_nodes`
- 只有首次构建或格式切换时，才重新使用 `initial_collapsed_nodes()`

---

## 5. Save 不变量

当前 save 路径是：

1. rewrite 到临时文件
2. 同路径时保留目标权限位
3. rename 回目标路径
4. reload 文档
5. 清空 tombstones / replacements / undo / redo
6. 刷新 inspector

因此当前没有 patch-save fast path。若未来要加，必须先证明收益足够大，且不破坏 replacement / tombstone / real delete 的边界。
