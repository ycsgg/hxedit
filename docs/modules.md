# Module Change Notes（高风险模块改动须知）

修改以下模块时要格外小心。每条都是历史上踩过坑、容易写错的地方。

---

## `src/core/document/*`

- 这是文档语义中心
- 改动前先想清楚：
  - display len
  - visible len
  - original len
  - tombstone / replacement / insert 之间的关系

## `src/core/piece_table.rs`

- 这里的任何“看似简单的优化”都可能破坏 `CellId` 稳定性
- 若改动了 piece 合并、split、restore，必须补回归测试

## `src/app/editing_state.rs` / `src/app/mode_state.rs` / `src/app/inspector_state.rs` / `src/app/undo.rs`

- 编辑行为和撤销语义在这里最容易出错
- 改完务必验证：
  - insert
  - delete
  - visual delete
  - paste
  - inspector edit
- Inspector 折叠态：`InspectorState.collapsed_nodes` 存 `NodePath`（`Vec<(struct_name, same-name sibling_index)>`）
  - `NodePath` 需要在 reparse 后尽量稳定，避免前面新增 / 删除同名 struct 时把折叠态串到别的节点
  - 改 `flatten` / `count_skipped` / path 生成时，保证折叠分支仍然按原有字段数前进 `field_idx`，否则字段编辑会定位到错误的字段
  - `refresh_inspector` 默认保留旧的 `collapsed_nodes`；只有首次或格式切换时才调用 `initial_collapsed_nodes`

## `src/app/clipboard_ops.rs`

- paste 既有语义问题，也直接影响用户体感
- 如果改变了 overwrite / insert 语义，必须同步 README 和测试
- copy / paste preview 若改动输出格式，记得同步命令提示、状态栏文案和测试

## `src/app/commands.rs` / `src/app/commands/*`

- `src/app/commands.rs` 只保留 command 提交与分发；具体执行按领域放在 `src/app/commands/*`
- 拆分命令实现时优先 move-only / helper 提取，不要把结构整理和命令语义变更混在一起
- `:hash` 命令使用流式哈希（`hash_logical_bytes`），64 KB 分块读取，不将全部数据加载到内存
- `:hash` 结果默认拷贝到剪贴板，状态栏显示 `[copied]`；剪贴板不可用时仍正常显示哈希值
- 文件搜索统一走 `:s [mode]<delim><pattern><delim>` / `:s! ...`；默认 `/text/` 是 UTF-8 bytes，`x/hex/` 是原始 hex bytes，`b` / `u32` / `u64` / `i32` / `i64` typed-value 都在同一入口解析；`:S` 只作为 deprecated hex-search 别名保留，命中时必须提示改用 `:s x/.../`
- `save` / `logical_bytes()` / `read_logical_range()` / `hash_logical_bytes()` / `for_each_logical_chunk()` / diff current-source / `transform_visible_range_in_place()` 都必须复用 `src/core/document/walk.rs`；不要在调用方重新手写 Original/Add + tombstone + replacement 分支
- active selection 不再只等于 Visual；command 从 inspector 进入时，当前字段范围也可作为 copy / export / replace / hash 的选区来源
- 搜索结果使用 "display 0x{:x}" 明确标注为 display offset
- 文件 `search_forward` / `search_backward` 按文档脏净分流：clean 文档（无 tombstone / replacement）走 `search_clean_forward` / `_backward` 的 SIMD `memchr::memmem::find` / `rfind`，按 chunk + `pattern.len()-1` overlap 续接跨边界匹配。dirty 文档走 `walk_visible_cells` / `walk_visible_cells_reverse`：walker 仍逐 chunk 判定脏净，clean chunk 走 `scan_clean_chunk_forward` / `_backward`（memmem + 首尾 `P-1` 字节 KMP 衔接），只有真正含 tombstone / replacement 的 chunk 才逐 cell KMP（维持 tombstone gap 与 replacement 覆盖语义）。跨 piece / 跨 chunk 边界续接靠跨 chunk 共享的 `KmpMatcher`，clean-chunk helper 必须保持"等价于逐字节 feed 整块"语义并在结尾 feed 末尾 `P-1` 字节为下个 chunk 续接。clean 文档快路径单次读用 `read_logical_range`，chunk 取 `min(SEARCH_CHUNK, max_contiguous_read_len())`，别退回整段读或纯 KMP
- `:g` 成功反馈当前使用 `moved ±0x... → 0x...`，别退回成只显示目标 offset
- copy 在 display span 与 logical bytes 不同时同时显示两者
- `:fill` / `:zero` 当前采用 overwrite 语义，不做 append；越过 EOF 时要明确提示 truncation；clean range 走 compact bulk undo + `ReplacementStore` pattern range overlay，并把覆盖范围整体视为 changed（不为 no-op 精确性扫 base bytes）；复杂 range 回退 per-byte undo；不要回退成先生成完整 pattern buffer 再 paste
- `:export` 始终导出 logical bytes；如果 display span 与逻辑字节数不同，状态栏文案要区分清楚；binary 导出走 `Document::for_each_logical_chunk` 边读边写 `BufWriter`，不要回退成 `logical_bytes` 整段物化（C array / Python bytes 文本导出仍可整段）
- `:xor` 只复制 active selection 的 XOR 后 logical bytes；`:xor!` 是 replacement 语义原地覆盖，不做 real delete / insert；key 不带 `0x` 时按十进制解析；`:xor!` clean range 走 compact bulk undo + `ReplacementStore` xor range overlay，复杂 range 回退 per-byte undo；不要回退成整段读出
- 任何按 chunk 读 original 的路径都应通过 `Document` walker，或显式复用 `Document::max_contiguous_read_len()` 裁剪单次读长，避免小 cache 配置下过度换页；`PageCache::read_range` 本身必须能安全组装跨 cache-capacity 的范围
- `:re` 使用与 `:s` 相同的 `[mode]<delim><needle><delim><replacement><delim>` 解析（旧 `hex/ascii <needle> -> <replacement>` 仍兼容），默认只能做等长 replacement；命中数超过 65535 时要求用户用 `--force` 二次确认，确认后按批次扫描 / 应用，clean 连续命中走 compact bulk undo + `ReplacementStore` pattern range overlay，dirty/复杂 run 可退回 per-byte undo；`:re!` 才允许 real delete + insert，仍走变长路径，别把两者写混
- `:re!` 在 visual selection 上执行后应退出 Visual，避免旧 display range 在长度变化后继续悬空
- 当前 `:diff` 已落地，并且必须继续保持为“只读同步滚动 side panel”：current side 是 current logical bytes，other side 是对比文件 raw bytes；打开时不要全文件预扫描，不要写入 `Document`，不要进入 undo / save
- `:diff` 可见页会在 `max_shift` 范围内做局部重对齐，避免一个 insert / delete 后把后续整页全部染成 replace；不要为了修局部对齐退回打开时全文件扫描
- `:diff` 着色语义：相同字节只在右侧用灰色；同位置字节不同左右两侧都用 warning 亮黄色；current-only 时右侧补 `__` 且用 error 红色；other-only 时左侧补 `__` 且用 error 红色
- `diff` overlay 必须先把可见 display slot 映射到 logical offset，再读 other raw offset；因为 tombstone 会让 logical range 不等于连续 display range
- `diff` 局部重对齐投影里的 other-only `__` 必须作为可见 cell 计入 hit-test / 选区映射；左侧点击 `__` 锚到相邻 current display slot，右侧 diff panel 点击要同步左侧光标并高亮对应 other raw byte
- 关闭或切走 diff panel（Tab 隐藏、`:diff off`、`:data` / `:insp` / `:sym` 打开其他 side panel）必须停止 diff 投影，左侧不能残留 diff 着色或 `__`
- 编辑 / undo / redo / save reload 后 diff 可按当前可见页即时重新着色，不能自动重扫大文件；`diff next` / `diff prev` 的远距离 mismatch 查找必须保持大块 stepper/progress，扫描中阻止其它输入且 Esc 可取消，不能退回无反馈的单次同步全文件扫描；后续做 shift-aware alignment cache 时继续保持这个边界
- 当前 `:dis` 已落地，并且必须继续保持为“主视图切换”而不是新的编辑语义；反汇编结果只能是当前 bytes 的投影，不能借机改写 replacement / tombstone / real delete 边界
- `sagitta-analysis` 下的 `:ana` 必须只读取 current logical bytes：用 `Document::for_each_logical_chunk` 分块物化，先检查 `visible_len() <= 128 MiB`，不要用会把 tombstone 读成 `0x00` 的 render-ish 路径。Sagitta snapshot 属于 App/UI 层 owned data，不写入 `Document`，不参与 undo / save / search
- Sagitta editing invalidation 挂在 App undo/edit 边界：replacement-only 标记 `OutdatedBytes`，insert / real delete / tombstone / resize replace 标记 `InvalidLayout`。不要为了追踪函数入口引入新的 document anchor 语义
- 当前已有统一 disassembly backend 抽象，按架构路由（`src/disasm/backend/registry.rs`）：x86/x86_64 用 `IcedX86Backend`、aarch64 用 `YaxpeaxArmBackend`，均为纯 Rust、无 C 依赖；后续扩展时，App / render / cache 仍不应直接绑死任一单一库 API。新增 backend 时实现 `DisassemblerBackend::decode_one` 并在 registry 注册，注意把架构特有的文本风格（如 yaxpeax PC-relative）归一化到既有显示/symbolize 管线
- 继续推进 disassembly 相关能力时，请同步维护 `disassemble-design.md` 中的阶段边界，不要把 detect / backend / render / editing / patch 一次性揉成一个超大提交

### Memory 模式

- memory 模式的 `:ms` / `:ms!` 是跨 region 搜索，不要复用或覆盖普通文件搜索的 `last_search`；命中状态与跳转都应显示 VA，而不是 display offset；side panel 的选中 region 不等于当前已打开 memory document 的 region，不要用面板选中态推导当前 document base VA
- 跨 region 字节匹配走 `memchr::memmem::find` / `rfind`（`search_bytes_forward` / `_backward`），分块 + `pattern.len()-1` overlap 不变，别退回逐字节 KMP。`:ms` / `:ms!` 命中后存 `last_memory_search`，`gn` / `gN` 经 `map_key_with_prefix` 的 `g` 前缀状态机触发 `repeat_memory_search`；这套 repeat 历史与文件搜索 `n` / `p`（`last_search`）必须保持独立，互不覆盖
- memory 模式的 `:mem freeze` / `:mem thaw` 是目标进程暂停/恢复控制；freeze 需要拒绝 pid 1 和当前 hxedit 进程，session drop 时对 frozen 状态做 best-effort thaw，commit 在目标仍 running 时应保留 warning 提示
- memory 模式下 `:w`（无 path）等价 `:mem commit`，`:w <path>` 必须拒绝并提示改用 `:export`；不要让 fixed-size memory document 走普通 `Document::save`。`MemoryRuntime.region_edits` 按 region index 持久化离开 region 时的 `(replacement_spans, undo, redo, cursor)`，切换 region 前要 `stash_opened_region_edits`、进入时用 `Document::apply_replacement_spans` 重放——不要回退成切换即清空 undo/replacement。`:mem commit-all` 必须真正按 region.start(VA) 升序遍历所有 dirty region，首个失败停下并保留其余 dirty；`:mem commit`（单 region）成功后才清空该 region 的 undo/redo 与 region_edits 条目
- memory 模式下 `:q` 必须用 `memory_dirty_summary()` 聚合（当前 document + 所有 region_edits）判定，任意 region dirty 时拒绝并汇总 `N regions dirty, total M bytes`；`:q!` 丢弃。`apply_replacement_spans` 是 replacement-only、bounds-checked，不得触发 insert / tombstone / real delete
- `:mem info` 走 `memory_info_text()` 聚合：选中 region + fingerprint、选中 region dirty 字节 + undo/redo 深度（opened region 取实时 undo/redo 栈，其余取 region_edits 快照）、session 级 dirty region 数与总字节、dirty region 列表（标 stale-base）、backend ro/rw、freeze 状态；多行用 `\n` 连接，不要退回只展示选中 region 的地址
- memory side panel 有 `MemoryPanelView` 三视图（Maps / ProcessList / Info），都走「1 行 header + 可滚动 body」并按 `scroll_offset` 切片，别再把 list/info 塞进单行 message。Maps 行区分 **selected（光标行，`▶`+inspector_active）** 与 **opened（已加载主视图，`●`+dirty 色）**，不可读行用 disasm_virtual 暗色、stale-base 用 warning，全部复用已有 palette 字段。鼠标点击只 `set_memory_selected_row` 改高亮（不自动 load/attach）；维护 `MEMORY_MAPS_HEADER_ROWS`（Maps body 区域 region 列表上方的非 region 行数）使 render 与 mouse hit-test 行号一致。`:mem list` 进 ProcessList 视图，仅 Enter 走 `attach_selected_memory_process`（含 `memory_dirty_summary` dirty 守卫，经真实 `open_backend_for_pid`）
- 平台 backend 在 `src/memory/platform/`：`mod.rs` 只做 `open_backend_for_pid` / `open_backend_for_process` / `list_processes` 的 `cfg(target_os)` 分发，具体实现按平台放独立文件（`linux.rs` = procfs + `kill -STOP/-CONT`；`windows.rs` = `windows-sys` 的 `OpenProcess` / `ReadProcessMemory` / `WriteProcessMemory`(+`VirtualProtectEx` 临时放开只读页) / `VirtualQueryEx` / ToolHelp 枚举 / `GetProcessTimes` fingerprint / freeze=thaw 用 **未公开** ntdll `NtSuspendProcess`/`NtResumeProcess`，经 `GetProcAddress` 动态解析，注释已标注）。macOS 仍走 `not(any(linux, windows))` 的 `MemoryUnavailable` 占位。新增平台只实现 `MemoryBackend` + 该平台 `list_processes`，不要改 trait 语义或 session 层；`windows-sys` 是 windows target 专属依赖（`memory` feature 仍为空 feature，platform mod 本身已被 `cfg` gate），改依赖记得同步 `licenses/THIRD_PARTY_NOTICES.txt`。Windows 侧验证靠 `cargo check/clippy --target x86_64-pc-windows-msvc --no-default-features --features memory`（C 依赖如 capstone 无法交叉编译，必须 `--no-default-features`）+ CI windows-latest；读写自身进程的最小测试无需提权

## `src/view/status.rs` / `src/app/render.rs` / `src/app/render/*`

- `src/app/render.rs` 只保留共享 render 类型、小 helper 与模块声明；主视图、diff 投影、side panel 和 render 测试分别放在 `src/app/render/*`
- 状态栏涉及 display len / visible len / logical selection bytes 语义
- 改动时不要把 display span 和 logical byte count 混成一个数字
- wrap-around search 当前使用 notice 类状态，不要退回普通 info 提示
- 搜索“同屏所有命中”高亮当前走 render overlay，改动时保持与 cursor / visual selection / inspector highlight 可叠加
- 当前 disassembly view 已替换左侧主视图；继续改 render 时，保持 search / selection / cursor overlay 仍按 byte 语义工作
- diff overlay 顺序要低于 inspector / search / visual selection / cursor；cursor 与 visual selection 仍保持最高优先级
- hex 主视图顶部固定显示 byte 列头（默认 `00 01 02 ... 0F`）；改 render / mouse / viewport 行数时保持列头不滚动且不被当作数据行命中

## `src/app/events.rs` / `src/app/events/*`

- `src/app/events.rs` 只保留 `handle_action` 入口；分发、编辑入口、inspector edit 与 action 收尾分别放在 `src/app/events/*`
- 改 action dispatch 时重点复查 command return mode、side panel 输入隔离、EOF clamp 与错误状态清理
- 不要把输入状态机重新堆回单个超大文件；需要继续拆分时按行为领域小步移动并跑相关 App 测试

## `src/view/palette.rs`

- 颜色配置支持四级自动检测：truecolor (RGB) / 256色 (Indexed) / 16色 (ANSI named) / 无色
- `--no-color` 和 `NO_COLOR` 环境变量都会禁用颜色
- 改动调色板时必须同时更新四个级别的配色，不要只改一个级别
- `supports-color` crate 负责基础终端能力检测，但 Windows Terminal 等环境需额外环境变量检测
- `is_truecolor_terminal()` 在 `supports-color` 之前检查 `WT_SESSION`、`COLORTERM=truecolor|24bit`、`TERM_PROGRAM=vscode`，确保 Windows Terminal 等现代终端正确识别为 TrueColor
- inspector 当前选中字段、search hits 与 diff page mismatch 都会叠加高亮到 hex grid；改 palette / render 时保持 overlay 仍可与 cursor / selection 共存

## `src/format/*`

- "字段可编辑"不等于"结构安全"
- 对 PNG / ZIP 等结构一致性强的格式，提示修改可能会破坏结构
- `DataRange` 字段类型用于显示大数据块的起始/结束偏移量，不可编辑
- 枚举、flags 与格式自定义显示 / 编辑转换统一走 `FieldType::Custom` + `CustomCodec`；具体格式需要的双向转换逻辑应留在对应 `defs/*` 内
- 合成可读字段必须保持固定长度 replacement 写回；例如 ZIP `modified_at` 覆盖 `mod_time + mod_date`，TAR `mode` / `size` / `mtime` 写回同宽 octal ASCII，不要借机改变 entry layout 或自动修 checksum
- ELF 格式已包含 Program Header Table 解析，改动时注意 32/64 位和端序分支
