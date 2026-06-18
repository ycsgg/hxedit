# Macro / Script Execution Design

本文定义 hxedit 后续接入宏和脚本能力时的执行层边界。目标是让自动化复用现有 byte 编辑语义，而不是绕过
`Document`、`EditOp`、undo、save、search 等核心模型。

---

## 1. 目标

- 宏可以在 TUI 和 headless CLI 中复用同一套执行语义。
- 宏中的写操作继续明确区分 replacement、real insert、tombstone delete、real delete。
- 宏可以显式处理当前选区，并能从 Visual 选区或 inspector 当前字段继承选区。
- 宏执行可以形成可撤销的 undo step，且 undo / redo 不依赖 TUI mode。
- 后续脚本语言只作为 `ExecCommand` 的生成器，不直接持有 `Document` 可变引用。

## 2. 非目标

- 第一阶段不录制原始按键序列。
- 第一阶段不把 side panel、render、状态栏、terminal event loop、clipboard 写入执行层。
- 第一阶段不让宏操作 `:diff`、`:insp`、`:sym`、`:data` 这类 UI / 投影视图命令。
- 第一阶段不承诺脚本语言。先完成声明式宏，再评估可选 `scripting` feature。

---

## 3. 分层

### 3.1 `src/exec/*`

执行层是唯一允许自动化直接写 `Document` 的层。建议新增：

```rust
pub enum ExecCommand {
    Goto { target: ExecGoto },
    Select { range: ExecRange },
    ClearSelection,
    Read { scope: ExecScope },
    Hash { algorithm: HashAlgorithm, scope: ExecScope },
    Search { pattern: Vec<u8>, direction: SearchDirection, select: SearchSelect },
    Overwrite { offset: ExecOffset, bytes: Vec<u8> },
    Insert { offset: ExecOffset, bytes: Vec<u8> },
    Delete { scope: ExecScope, kind: DeleteKind },
    Fill { offset: ExecOffset, pattern: Vec<u8>, len: u64 },
    XorInPlace { scope: ExecScope, key: u8 },
    Replace {
        scope: ExecScope,
        needle: Vec<u8>,
        replacement: Vec<u8>,
        allow_resize: bool,
        force: bool,
    },
    ExportBinary { scope: ExecScope, path: PathBuf },
    Save { path: Option<PathBuf> },
    Undo { steps: usize },
    Redo { steps: usize },
}
```

`ExecCommand` 是宏和未来脚本的稳定 IR。它只描述数据操作，不描述 UI。

### 3.2 `ExecState`

把当前 `ExecSession` 里的运行状态拆成可复用结构：

```rust
pub struct ExecState {
    pub cursor: u64,
    pub selection: Option<ExecSelection>,
}
```

`ExecSession` 继续持有 `Document + Config + ExecState + undo/redo`，供 headless CLI 和测试使用。
`App` 通过 adapter 把自身状态投影到 `ExecState`，执行后再把 cursor / selection / outcome 接回 UI。

### 3.3 `src/automation/*`

建议新增自动化层，负责文件格式、批执行策略、录制状态：

- `automation/macro_file.rs`：TOML 宏文件解析与版本检查
- `automation/runner.rs`：把宏 step 转成 `ExecCommand` 并调用执行层
- `automation/recording.rs`：后续命令录制状态，不放进第一阶段也可以

自动化层可以依赖 `exec`，但 `exec` 不依赖自动化层。

---

## 4. 选区模型

### 4.1 一等执行状态

宏不录制 Visual mode，也不保存 selection anchor。宏只处理归一化后的范围：

```rust
pub struct ExecSelection {
    pub range: ExecRange,
}

pub struct ExecRange {
    pub space: RangeSpace,
    pub start: u64,
    pub len: u64,
}

pub enum RangeSpace {
    Display,
    Logical,
}
```

Display range 选择 display slot，可能包含 tombstone slot。Logical range 选择 save / export / hash 看到的逻辑字节流。

### 4.2 宏启动时的选区策略

宏文件头声明启动策略：

```toml
version = 1
selection = "inherit" # inherit | clear | require
undo = "group"        # group | per-step
on_error = "stop"     # stop | rollback
```

- `inherit`：TUI 调用时继承当前 active selection；headless 下等同于 `clear`。
- `clear`：启动时清空执行层 selection。
- `require`：启动时必须有 active selection，否则拒绝执行。

TUI adapter 的 active selection 来源：

- Visual 选区：转换成 `RangeSpace::Display`。
- Inspector 当前字段：转换成字段覆盖的 display range。
- 没有选区：`None`。

转换发生在宏启动时。宏执行过程中不再读取 `Mode::Visual`、inspector row、side panel focus 或 command return mode。

### 4.3 显式 selection 命令

宏内部必须用显式 step 修改选区：

```toml
[[steps]]
cmd = "select"
space = "display"
start = "0x100"
len = 32

[[steps]]
cmd = "clear-selection"
```

命令需要选区时应写 `scope = "selection"`。宏中不要沿用 TUI 命令的隐式行为，例如 `:hash` 没有选区时 hash 全文件。宏要写清楚：

```toml
[[steps]]
cmd = "hash"
scope = "all"
algorithm = "sha256"
```

或者：

```toml
[[steps]]
cmd = "hash"
scope = "selection"
algorithm = "sha256"
```

### 4.4 搜索与选区

搜索默认只移动 cursor，不改 selection：

```toml
[[steps]]
cmd = "search"
mode = "hex"
pattern = "de ad be ef"
direction = "forward"
select = "none"
```

需要把命中设为选区时显式声明：

```toml
[[steps]]
cmd = "search"
mode = "hex"
pattern = "de ad be ef"
select = "match"
```

`select = "match"` 产生 display selection，长度等于 pattern 的逻辑字节长度映射到的 display span。若命中跨 tombstone gap，使用实际匹配结果的起止 display offset。

### 4.5 编辑后的选区保留规则

宏执行层必须在每个写命令后按以下规则处理 selection：

| 操作类型 | 例子 | Selection 规则 |
|---|---|---|
| cursor-only | `goto`、`search select=none` | 保留 |
| 显式 selection | `select`、`clear-selection`、`search select=match` | 按命令结果更新 |
| replacement-only | `overwrite`、`fill`、`xor!`、等长 `replace`、inspector 固定宽字段写 | 保留 |
| tombstone delete | 普通 delete / visual delete 语义 | display selection 保留，logical selection 清空 |
| real insert / real delete | insert paste、insert-mode backspace、变长 `replace!` | 清空 selection |
| save / export | `save`、`export-binary` | 保留，但 save 成功后 undo/redo 清空 |

原因：

- replacement-only 不改变 display / logical 长度，保留 selection 是稳定的。
- tombstone delete 保留 display slot，但 logical offset 会变化，因此 logical selection 必须清空。
- real insert / real delete 会移动后续 display offset，第一阶段不引入 anchor 或 CellId selection，直接清空最安全。

---

## 5. Scope 与 offset

宏命令中所有读写范围都走 `ExecScope`：

```rust
pub enum ExecScope {
    Selection,
    Range(ExecRange),
    All,
}
```

规则：

- 写命令不得默默把缺失 selection 扩大成全文件。
- 大范围读写继续走 `Document::for_each_logical_chunk`、`hash_logical_bytes` 或 walker。
- 任何 JSON / TOML outcome 都不得为了方便把大文件整段物化。
- `All` 对 dirty 文档表示当前 logical bytes 全量，不表示 original raw file。
- `read`、`hash`、`search` 可以通过可选 `id` 把结果绑定给后续 step 使用；`read` 仍拒绝 `scope = "all"`，需要读取时必须显式给出 selection 或 range。

Offset 表达式第一阶段只支持绝对 offset、`cursor`、`end` 与相对 cursor：

```toml
offset = "cursor"
offset = "cursor+0x10"
offset = "0x100"
offset = "end"
```

复杂表达式留给脚本层，不进入宏 MVP。

---

## 6. Undo / redo 与批执行

### 6.1 Undo step

执行层新增与 App mode 无关的 step：

```rust
pub struct ExecStep {
    pub cursor_before: u64,
    pub selection_before: Option<ExecSelection>,
    pub cursor_after: u64,
    pub selection_after: Option<ExecSelection>,
    pub ops: Vec<EditOp>,
}
```

`EditOp` 仍是唯一可逆编辑记录。`ExecStep` 只补 cursor 和 selection 状态。

### 6.2 批执行策略

宏默认 `undo = "group"`：

- 一个宏形成一个 undo step。
- 多个写命令的 `EditOp` 按执行顺序合并。
- undo 时反向回放 `EditOp`，然后恢复 `cursor_before` 和 `selection_before`。

`undo = "per-step"`：

- 每个产生有效 `EditOp` 的 step 单独入栈。
- 适合调试和录制宏。

没有编辑效果的 step 不入 undo 栈。

### 6.3 Error 策略

`on_error = "stop"`：

- 默认策略。
- 出错时停止，已成功执行的编辑保留，可用 undo 撤回。

`on_error = "rollback"`：

- 只允许宏不包含 `save`、`export`、memory commit 等外部副作用时启用。
- 出错时用已收集 `EditOp` 回滚到宏开始前。
- 回滚失败必须返回错误并保留当前 document 状态，不得伪装成功。

`save` 是 undo barrier：

- 保存成功后沿用当前 save 不变量，清空 tombstone / replacement / undo / redo 并 reload document。
- 宏中 `save` 之后不能再把之前的编辑放在同一个 undo group。

---

## 7. 宏文件格式

MVP 使用 TOML，复用现有依赖，不新增脚本运行时：

```toml
version = 1
selection = "inherit"
undo = "group"
on_error = "stop"

[[steps]]
cmd = "select"
space = "display"
start = "0x100"
len = 16

[[steps]]
cmd = "xor"
scope = "selection"
key = "0xaa"
in_place = true

[[steps]]
cmd = "hash"
scope = "selection"
algorithm = "sha256"
```

解析规则：

- `version` 必填，当前只接受 `1`。
- `cmd` 必填，未知字段报错。
- 数字字段接受十进制或 `0x` 前缀。
- bytes 字段接受 literal hex stream，例如 `"de ad be ef"`，也接受变量引用：
  `bytes = { from = "payload_sha256", format = "bytes" }`。
- `read`、`hash`、`search` 可选 `id`。`read` 绑定 logical bytes；`hash`
  绑定 digest bytes 与 compact hex text；`search` 命中时绑定匹配 pattern bytes。
  后续 `bytes`、`pattern`、`needle`、`replacement` 等字节字段可用
  `{ from = "...", format = "bytes|hex-text" }` 引用。`format` 默认是 `bytes`。
- 路径字段相对宏文件所在目录解析，除非是绝对路径。

命令命名使用宏自己的稳定名字，不直接复用 `:` aliases。这样 `:re`、`:replace`、`:S` 等用户输入兼容别名不会污染宏格式。

---

## 8. TUI 接入

已落地的自动化入口：

```text
:source <path>
:script <path>
```

`:source` 语义：

1. App adapter 读取当前 active selection，转换成 `ExecSelection`。
2. 解析宏文件。
3. 调用自动化 runner。
4. 根据 `ExecBatchOutcome` 更新 cursor、selection、undo/redo、dirty、inspector、disassembly cache、Sagitta invalidation、status。

App 层仍负责：

- 状态栏文案
- mode clamp
- inspector refresh
- data panel refresh
- diff stale 标记
- disassembly cache invalidation
- Sagitta invalidation

执行层不接触这些 UI 状态。

`:script` 语义与 `:source` 共用 App adapter：启动时继承 active selection，脚本通过 `hx_`
host API 生成 `ExecCommand`，执行后把 cursor / selection 接回 TUI。脚本编辑在 TUI 中按一次
`:script` 命令聚合成一个 undo step；如果脚本调用 `hx_save()`，保存前 undo 按现有 save
不变量清空。

录制命令后续再加：

```text
:macro record <name>
:macro stop
:macro run <name>
```

录制只记录已经提交并成功执行的 exec-compatible 命令。第一版不录制裸按键，不录制 side panel navigation，不录制 render 状态。

---

## 9. Headless CLI 接入

已落地：

```bash
hxedit file.bin --run patch.hxmacro
hxedit file.bin --script patch.hxscript
hxedit file.bin --command "script patch.hxscript" --command "w"
hxedit file.bin --command "goto 0x100" --command "fill 90 16" --command "w"
hxedit file.bin --select display:0x100:16 --run patch.hxmacro
```

Headless 路径直接打开 file-backed `Document` 并创建 `ExecState`，不创建 `App`。当前行为：

- 默认输出人类可读 summary。
- 只支持文件目标；`--pid` / `--process` 仍走 TUI 内存编辑路径。
- `--select display:<start>:<len>` 或 `--select logical:<start>:<len>` 作为初始选区。
- 所有 `--run` 文件先执行，然后执行所有 `--script` 文件，最后执行所有 `--command`
  字符串；三个组内各自保持顺序。
- 修改只在宏、脚本或命令显式执行 `save` / `hx_save()` / `w` / `wq` 后落盘。
- `--command` 中 `hash`、binary `export`、`replace` 有 `--select` 时作用于该选区，
  否则作用于全文件；`xor!` 必须显式提供选区。
- `--command` 只接受可映射到执行层的命令：`w`、`wq`、`source`、`script`、`fill`、`zero`、
  `goto`、binary `export`、`xor!`、`replace`、`search`、`hash`。
- `:diff`、`:insp`、`:sym`、`:data`、clipboard paste/copy 等 UI-only 命令拒绝执行。

后续仍可补 `--json`，输出结构化 `ExecOutcome` / `ExecBatchOutcome`。

---

## 10. 脚本层

已加入 Rhai demo，入口是 TUI `:script <path>` 或 headless `--script <path>` / `--command "script <path>"`。当前只提供一组 `hx_` 前缀 host
API，并继续满足：

- 放在可选 feature，例如 `scripting`。
- 脚本只能调用 host API，host API 只生成并执行 `ExecCommand`。
- 脚本不能拿到 `&mut Document`。
- 脚本读取大范围 bytes 必须有预算。当前默认：总读取 `512 MiB`、单次读取 `64 MiB`、
  单个 bytes blob `64 MiB`。
- 脚本步骤数、读字节数、写字节数、输出 artifact 数都要有上限。

当前 demo Host API：

```text
hx_cursor()
hx_len_display()
hx_hex(text)
hx_ascii(text)
hx_goto(offset)
hx_select_display(start, len)
hx_clear_selection()
hx_read_display(start, len)
hx_read_selection()
hx_search(pattern)
hx_hash_hex(algorithm)
hx_overwrite(offset, bytes)
hx_insert(offset, bytes)
hx_fill(offset, pattern, len)
hx_save()
```

脚本只是更灵活的宏生成器，不拥有新的编辑语义。

---

## 11. 测试要求

新增宏能力时至少覆盖：

- `ExecCommand` parser / TOML parser 的未知字段、缺参、数字解析、路径解析。
- inherited / clear / require 三种 selection 策略。
- display selection 与 logical selection 在 tombstone 后的不同处理。
- replacement-only 后 selection 保留。
- real insert / real delete / `replace!` 后 selection 清空。
- `undo = group` 和 `undo = per-step`。
- `on_error = stop` 保留已成功编辑，`on_error = rollback` 回滚。
- save 作为 undo barrier。
- hash / export 继续走 streaming path，不整文件物化。
- TUI adapter 从 Visual selection 和 inspector field selection 继承范围。

若新增宏写操作，固定 seed document fuzz 也应补同构模型操作。

---

## 12. 推荐落地顺序

1. 新增 `ExecCommand`、`ExecScope`、`ExecState`、`ExecStep`、batch runner。
2. 把 `fill`、`xor!`、`replace`、`delete`、`insert`、`overwrite`、`hash`、`export`、`search`、`goto` 逐步委托到 exec runner。
3. 给 `ExecSession` 接入 undo / redo 和 `execute_batch`。
4. 新增 TOML 宏 parser 和 `:source <path>`。
5. 新增 headless `--run` / `--command`。
6. 新增 headless Rhai `--script` demo。
7. 新增录制命令。

每一步都应保持 App 层原行为不漂移。结构移动和编辑语义变更不得混在同一个阶段。
