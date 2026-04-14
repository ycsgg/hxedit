# Insert Mode + PieceTable Design

## 目标

为 `hxedit` 增加真正的插入模式，并用 `PieceTable` 支撑“插入立即改变后续偏移”的语义。

当前实现的核心问题是：

- 覆盖写、删除、追加都依赖 `PatchSet`
- 普通删除是占位删除，不立即影响后续偏移
- 追加只是尾部特判，不是通用插入
- 一旦需要“中间插入”，当前 offset 模型会立刻失真

本设计的目标不是小修，而是明确一套可以支撑插入模式的坐标和数据模型。

---

## 已确认的行为约束

这些是实现时必须遵守的硬约束：

### 1. 普通删除保持现状

- normal / visual 模式下按 `d` 或 `x` 的删除逻辑先不改
- 删除仍然是 **占位删除**
- 被删除的单元仍占显示槽位
- 当前 UI 中 `XX` / `x` 的显示逻辑保留
- 普通删除不会立即让后续 offset 左移

### 2. 只有 Insert mode 下的 `Backspace` 是“真实删除”

- `Insert mode` 中按 `Backspace`
- 直接删除当前插入流中的前一个逻辑单元
- 该删除会立刻影响后续偏移

### 3. 插入模式的单字符输入规则

输入 hex 字符时按 nibble 行为处理，但要满足：

- 输入第一个 hex 字符，例如 `a`
  - 立即在当前位置插入一个临时字节
  - 显示为 `a0`
  - 这里的低 nibble `0` 是 **虚拟的**
  - 这一步已经立刻影响后续偏移
- 再输入第二个 hex 字符，例如 `b`
  - 不新插入字节
  - 而是把刚才的 `a0` 改成 `ab`
- 如果此时退出插入模式
  - 那个虚拟的 `0` 固化为真实低 nibble
  - `a0` 成为真实字节

这意味着：

- 插入模式不能简单沿用当前 `EditHex { phase }`
- 必须有显式的“待补全低 nibble”的插入状态

---

## 总体模型：混合 PieceTable + Tombstone

这次不能把所有删除语义都切到 PieceTable，因为普通删除必须保持占位显示。

因此需要一个混合模型：

### 层 1：Original Buffer

- 原文件只读视图
- 由 `FileView` 提供

### 层 2：PieceTable

负责：

- 插入内容
- Insert mode 下 `Backspace` 的真实删除
- 所有“立即影响偏移”的编辑

不负责：

- 普通占位删除

### 层 3：Tombstone Layer

负责：

- normal / visual 删除产生的占位删除
- 渲染时显示 `Deleted`
- 保存时跳过这些单元

---

## 坐标体系

### 关键原则

以后所有交互层使用的 offset 都必须是：

- **Display Offset / 布局偏移**

它具有以下特点：

- 会受插入影响
- 不会因为普通 tombstone 删除而收缩
- 用于：
  - 光标
  - viewport_top
  - goto
  - 搜索结果
  - visual 选区
  - copy / paste 目标位置
  - 状态栏显示 offset

### 不能再直接用原始文件 offset 的地方

以下逻辑都不能继续拿原始文件 offset 当主坐标：

- `Document::byte_at`
- `row_bytes`
- `search_forward / search_backward`
- `selection_range`
- `paste`
- `copy`
- `goto`

否则插入后，显示偏移和逻辑偏移会立刻错位。

---

## PieceTable 设计

建议新增：

```rust
pub enum PieceSource {
    Original,
    Add,
}

pub struct Piece {
    pub source: PieceSource,
    pub start: u64,
    pub len: u64,
}

pub struct PieceTable {
    original_len: u64,
    add_buffer: Vec<u8>,
    pieces: Vec<Piece>,
}
```

### PieceTable 提供的核心操作

- `len()`
- `insert_bytes(display_offset, bytes)`
- `delete_range_real(display_start, len)`
- `byte_at(display_offset)`
- `range(display_offset, len)`
- `split_piece_at(display_offset)`

### 设计要求

- 支持头部 / 中部 / 尾部插入
- 支持真实删除任意逻辑区间
- 支持高频小插入
- 逻辑 offset 查询结果稳定

---

## Tombstone 设计

当前 `PatchSet::Deleted` 不能继续简单按“原始 offset”记，因为插入后显示偏移会变化。

### 正确做法

tombstone 不应绑定原始 offset，而应绑定一个更稳定的“显示单元标识”。

推荐引入：

```rust
pub enum CellId {
    Original(u64),
    Add(u64),
}
```

这样：

- 原文件字节有 `CellId::Original(offset)`
- add buffer 中的字节有 `CellId::Add(offset)`
- tombstone 集合记录的是 `CellId`

好处：

- 插入导致 display offset 变化时，tombstone 不会指错对象
- copy/save/search 可以先把 display offset 解到 `CellId` 再决定行为

---

## Document 新职责

`Document` 将不再是“FileView + PatchSet overlay”，而会变成：

- `FileView`
- `PieceTable`
- `TombstoneSet<CellId>`
- 编辑/搜索/保存桥接层

### Document 读接口

- `len()`
- `byte_at_display(offset) -> ByteSlot`
- `row_bytes_display(offset, width)`
- `logical_bytes(start, end)`

### Document 写接口

- `insert_byte(offset, value)`
- `insert_bytes(offset, bytes)`
- `delete_range_tombstone(start, end)`
- `delete_range_real(start, len)`
- `replace_display_byte(offset, value)`

### ByteSlot 语义

建议继续保留：

```rust
enum ByteSlot {
    Present(u8),
    Deleted,
    Empty,
}
```

因为普通删除仍然需要 `Deleted`。

---

## Insert Mode 设计

新增模式：

```rust
Mode::InsertHex {
    pending: Option<PendingInsert>,
}
```

建议 `PendingInsert`：

```rust
struct PendingInsert {
    offset: u64,       // 这个字节在当前 display 流中的位置
    high_nibble: u8,   // 第一个输入的 hex
}
```

### 进入方式

- `i`：进入 `InsertHex`
- 当前光标表示“在该 display offset 前插入”

### 输入第一个 hex 字符

例如输入 `a`：

- 在当前 `cursor` 位置插入字节 `0xa0`
- 记录 `pending = Some { offset: cursor, high_nibble: 0xa }`
- 光标移动到“该插入字节之后”
- 显示结果是 `a0`
- 其中 `0` 只是虚拟低 nibble

### 输入第二个 hex 字符

例如再输入 `b`：

- 找到 pending 对应的那个刚插入的字节
- 把它从 `0xa0` 改成 `0xab`
- 清空 pending
- 光标保持在该字节之后，准备下一次插入

### 退出 Insert 模式

若 `pending` 存在：

- 不撤销
- 直接固化这个 `a0`
- 也就是低 nibble `0` 成为真实值

退出触发包括：

- `Esc`
- 切换到 normal / visual / command
- 鼠标点击跳转
- 执行搜索 / goto / save 之前

---

## Insert mode 下 Backspace

这是唯一允许“真实删除”的交互。

### 规则

#### 情况 A：当前有 pending 半字节

例如刚输入了 `a`，显示 `a0`：

- `Backspace` 直接删除这个刚插入的字节
- `pending = None`
- 光标回到插入前位置
- 这是一次真实删除，后续偏移立即左移

#### 情况 B：当前没有 pending

- 删除光标前一个 display 单元
- 这个删除也是真实删除
- 如果前一个单元是原文件字节，也要通过 piece table 删除
- 如果前一个单元是插入字节，同样 piece table 删除

### 与普通删除的区别

- normal/visual 删除：占位删除，不收缩
- insert backspace：真实删除，立即收缩

---

## 普通删除保持不变

### normal / visual 删除

保持现状：

- 生成 tombstone
- 渲染时显示为 `Deleted`
- display 流长度不变
- offset 不收缩
- 保存时跳过这些单元

这也是为什么需要 PieceTable 与 Tombstone 共存。

---

## 搜索

搜索必须基于 **display stream**。

### 规则

- 插入内容可搜索
- 普通 tombstone 删除的单元不应匹配
- 但它仍然占显示位置

实现上：

- 搜索遍历 display stream
- display stream 中：
  - `Present(byte)` 参与匹配
  - `Deleted` 视为“不可匹配占位”

这样：

- 搜索结果 offset 和 UI 光标一致
- `n/p` 仍然正确
- 插入后结果位置立即更新

---

## Copy / Paste 语义

### Copy

- 按 display 选区取内容
- tombstone 单元默认不输出实际 bytes
- 插入字节正常输出

### Paste

继续保留当前命令：

- `:p` / `:paste [num]`
  - hex/base64 文本解析后插入
- `:p!` / `:paste! [num]`
  - raw bytes 插入
- `:p?` / `:paste? [num]`
  - 只预览，不修改

### Paste 语义

在 PieceTable 模型下，paste 应统一为：

- **插入**，不是覆盖

也就是：

- 在当前 display offset 前插入解析结果
- 后续偏移立即右移

如果要保留旧的覆盖式 paste，需要新增独立命令；当前设计不建议混在 `:p` 上。

---

## 保存

统一 rewrite：

- 遍历 display stream
- `Present(byte)` 写出
- `Deleted` 跳过
- piece table 中真实删除过的内容天然不存在

不再做：

- in-place save fast path

原因：

- PieceTable + Tombstone 混合后，rewrite 是最稳的正确实现
- 先保证语义正确，再考虑优化

---

## Undo

当前 undo 已经是 transaction 模型，应继续沿用，但要扩展操作类型：

```rust
enum EditOp {
    Insert { offset: u64, bytes: Vec<u8> },
    RealDelete { offset: u64, bytes: Vec<u8> },
    TombstoneDelete { ids: Vec<CellId> },
    ReplaceInsertedByte { offset: u64, before: u8, after: u8 },
}
```

### 事务边界

- 一次普通删除 = 一个 transaction
- 一次 visual 删除 = 一个 transaction
- 一次 paste = 一个 transaction
- Insert mode 输入一个字符：
  - 建议先作为一个 transaction
  - 但“第一字符插入 a0 + 第二字符补成 ab”最好在实现上合并成一个可撤销单元

建议规则：

- 若第二个字符是在同一 insert 会话中补同一个 pending 字节
- 则这两个动作应合并为一个 undo action

否则用户按一次 `undo` 只能从 `ab` 回到 `a0`，体验不对。

---

## 实施阶段

### Phase 1：引入 PieceTable

新增：
- `src/core/piece_table.rs`

完成：
- piece 定义
- 逻辑 offset -> byte/range
- 插入与真实删除单测

### Phase 2：Document 读路径切换

完成：
- `len()` 基于 piece table
- `byte_at` / `row_bytes`
- display stream 生成
- 保持 tombstone 仍可 overlay

### Phase 3：Insert mode 状态机

完成：
- `Mode::InsertHex`
- pending nibble 状态
- `a -> a0` 虚拟低 nibble
- `a + b -> ab`
- `Esc` 固化 `a0`

### Phase 4：Backspace 真实删除

完成：
- Insert mode Backspace 真实删除
- display offset 立即左移
- undo 正确回放

### Phase 5：搜索 / copy / paste / save 接入新流

完成：
- 搜索基于 display stream
- copy 基于 display stream
- paste 做插入
- save rewrite

### Phase 6：收口与测试

完成：
- 状态栏 / mode label / cursor 细节
- 鼠标点击与 visual 选区在插入后仍正确
- 全量测试通过

---

## 必测场景

### Insert 基础
- 在中间插入一个 nibble：`a -> a0`
- 第二个 nibble 补齐：`a0 -> ab`
- `Esc` 固化 `a0`

### 偏移变化
- 中间插入后，后续光标/搜索/选区偏移右移
- Insert Backspace 后，后续偏移左移

### 与 tombstone 共存
- 原文件某字节 tombstone 后仍占位
- 在其前后插入时显示正确
- 保存时同时处理 tombstone 跳过 + 插入写出

### Undo
- `a` 后 undo 回到插入前
- `a` 再 `b` 后 undo 一次回到插入前，而不是 `a0`
- paste 一次 undo 全撤销
- visual 删除一次 undo 全恢复

### 搜索
- 可命中插入内容
- 不命中 tombstone 占位
- 插入后 `n/p` 仍正确

### 保存
- 插入后文件内容正确
- 普通删除仍跳过导出
- insert-backspace 的真实删除不写出

---

## 当前结论

这次实现不能简单理解成“把 patch 换成 piece table”。

正确目标是：

- **PieceTable 处理实时结构变化（插入、insert-backspace）**
- **Tombstone 保留旧删除语义**
- **Display offset 成为统一交互坐标**

只有这样，才能同时满足：

- 插入立刻影响偏移
- 删除占位显示不变
- 半 nibble `a0` 的虚拟低位行为正确
