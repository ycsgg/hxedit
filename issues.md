# hxedit Issues



# To be fixed (bug)

- [ ] **[P0] 覆盖粘贴在 EOF 后直接截断，与 README 描述和旧测试预期不一致**
  - 现状：`src/app/clipboard_ops.rs::apply_paste_overwrite()` 到 EOF 后直接 `break`
  - 影响：用户预期不稳定；README 仍写着“覆盖并在超出 EOF 时追加”
  - 处理建议：统一语义，二选一
    - A. 实现“前半段 overwrite，超出部分 insert”
    - B. 明确改 README、命令提示、测试，声明 overwrite 模式不追加

- [x] **[P0] `src/app/tests.rs` 当前未接入测试编译，且内容已过期**
  - 已完成：`src/app.rs` 已接入 `#[cfg(test)] mod tests;`
  - 已完成：失效的 `apply_paste_bytes` 引用已清理，改为当前真实 API 的可执行回归测试
  - 结果：App 层基础交互测试不再是“文件存在但没跑”的假覆盖

- [ ] **[P0] Inspector 在窄终端下可能进入有焦点但不可见的状态**
  - 现状：状态机会进入 `Mode::Inspector`，但渲染层只有宽度足够时才真正绘制 inspector
  - 影响：用户会“进入 inspector 模式但看不到 inspector”
  - 处理建议：宽度不足时拒绝进入、自动降级显示方案、或给出明确状态提示

- [ ] **[P0] ZIP inspector 未正确处理 data descriptor / 非直接可得的压缩大小**
  - 现状：扫描 local file header 时直接依赖 local header 中的 `compressed_size`
  - 影响：遇到 bit flag `0x0008` 时，结构推进 offset 可能错位
  - 处理建议：补 data descriptor / central directory 感知逻辑，至少先做保护性校验

- [ ] **[P1] `:undo` / 命令式 undo 后，Normal 模式下可能残留 EOF 光标**
  - 现状：undo 先按旧 mode clamp 光标，再把 mode 强制切回 `Normal`
  - 影响：模式与光标边界规则可能短暂失配
  - 处理建议：`restore_mode == false` 时，最终再按 `Mode::Normal` 重新 clamp 一次

- [ ] **[P1] README 声称支持 patch-in-place save，但当前实现实际总走 rewrite save**
  - 现状：`Document::save()` 直接调用 `save::save_rewrite()`
  - 影响：文档与实现不一致，也影响对大文件 overwrite-only 编辑的性能预期
  - 处理建议：要么实现真实 patch-save fast path，要么下调 README 表述

- [ ] **[P1] PNG / ZIP inspector 编辑后缺少结构一致性保护**
  - PNG 例子：修改 chunk 长度、IHDR 字段后，没有 CRC/结构合法性修复与校验
  - ZIP 例子：修改 header 字段后，没有一致性检查
  - 影响：UI 看起来“可编辑”，但结果可能直接生成结构损坏文件
  - 处理建议：在提交前做风险提示


# Feature

- [ ] **[P1] 命令历史**
  - 目标：命令模式支持 Up / Down 浏览历史
  - 建议：支持最近一次命令编辑恢复，不要每次进入都丢掉上下文

- [ ] **[P1] Redo**
  - 当前只有 undo，没有 redo
  - 建议：与编辑、插入、删除、粘贴、inspector 编辑共用统一操作栈语义

- [ ] **[P1] 搜索环绕 / wrap-around**
  - 当前找不到就结束
  - 建议：支持配置为“到尾后从头继续”“到头后从尾继续”

- [ ] **[P1] 原始二进制 copy**
  - 当前已有 raw paste，但 copy 仍以文本格式化为主
  - 建议：补“复制原始 bytes 到系统剪贴板”的能力

- [ ] **[P2] 更丰富的配色 / 主题**
  - 目标：支持更明显的 cursor / selection / deleted / replacement / format highlighting 区分

- [ ] **[P2] 更多内置格式定义与更深层结构视图**
  - 可继续扩展 ELF / PNG / ZIP，也可增加更多格式
  - 建议：优先做“结构稳定、字段可验证”的格式


# Imporve (体验改进)

- [ ] **[P0] 打开无写权限文件时自动降级为 readonly，并给出明确提示**
  - 当前默认按可写方式打开，失败就整体失败
  - 更合理的 UX：自动 fallback 到只读模式

- [ ] **[P0] 命令执行失败时保留 command buffer，不要先清空**
  - 现状：`submit_command()` 在执行前就清空输入
  - 影响：例如 `:q` 因 dirty 被拒绝时，用户输入也一起丢失
  - 建议：只有命令成功提交后再清空，失败时保留原命令供继续编辑

- [ ] **[P1] 状态栏区分 display length / visible length / logical selection bytes**
  - 当前 tombstone 存在时，用户容易混淆“显示长度”和“实际保存长度”
  - 建议：状态栏显式显示 `len / visible / sel(logical)` 等信息

- [ ] **[P1] Inspector 不可用时给更明确的交互反馈**
  - 包括：
    - 没检测到格式
    - 屏幕过窄无法展示
    - 当前格式只支持只读查看

- [ ] **[P1] Paste preview 展示更多上下文**
  - 建议展示：
    - 解析来源（hex/base64/raw）
    - 原始长度 / 截断后长度
    - 前若干字节预览
    - overwrite / insert 的预期效果

- [ ] **[P2] 搜索与复制的反馈更精确**
  - 建议区分：
    - 搜索命中的是 display offset 还是 logical bytes
    - copy 复制出的逻辑字节数与选区显示跨度


# Prof (性能优化)

- [ ] **[P0] `PieceTable::resolve()` 从线性扫描优化为更稳定的随机访问路径**
  - 当前很多操作仍会触发 O(pieces) resolve
  - 建议方向：前缀索引、块级索引、最近命中缓存、二分辅助表

- [ ] **[P0] 大选区复制 / 删除改为 piece 级批处理**
  - 当前路径偏逐字节，会频繁调用 `byte_at()` / `resolve()`
  - 建议直接按 piece 遍历，批量处理 tombstone / replacement

- [ ] **[P0] Inspector refresh 改成增量刷新**
  - 当前很多编辑后会重新 detect + parse + flatten
  - 建议只刷新受影响 field / struct，避免每次全量重建

- [ ] **[P1] 格式解析读字节改为块读取，不要大量逐字节 `byte_at()`**
  - 当前 detect / parse 中有不少逐字节读路径
  - 建议统一走批量读取 helper

- [ ] **[P1] `PageCache::touch()` 从 O(n) 降为 O(1)**
  - 当前命中路径仍要在线性结构里找页位置
  - 建议换成更直接的 LRU 结构

- [ ] **[P1] save slow-path 避免逐字节 `write_all(&[byte])`**
  - 当前 slow chunk 在替换/跳过 tombstone 时仍是逐字节写
  - 建议先写入临时缓冲，再批量输出

- [ ] **[P1] 为 overwrite-only 变更补真实 patch-save fast path**
  - 这既是实现正确性 / 文档一致性问题，也是重要性能优化项
  - 对大文件尤其关键


# Quality (实现规范性 / 代码质量整改)

- [x] **[P0] 建立基础代码风格门禁：先过 `cargo fmt --check`**
  - 已完成：当前 `cargo fmt --check` 可直接通过
  - 已完成：将其纳入提交前最小检查项说明
  - 约束：后续不要在逻辑改动 PR 中混入无关格式噪音

- [x] **[P0] 清理默认 clippy warning，至少做到 `cargo clippy --all-targets` 干净**
  - 已完成：默认级别 `cargo clippy --all-targets` 已无 warning
  - 后续建议：pedantic 警告仍按小步、分批处理，不做一口气大重写

- [x] **[P0] 接回或迁移失效的 App 层测试，清理未编译的陈旧测试文件**
  - 已完成：`src/app/tests.rs` 已接入编译并清理失效 API
  - 已完成：补回 overwrite paste / undo 等 App 行为的可执行测试
  - 后续：继续按真实交互路径补 paste / inspector / command 相关覆盖

- [x] **[P0] 统一错误边界：内部业务层尽量收敛到 `HxError/HxResult`**
  - 已完成：入口层继续保留 `main` / CLI / TUI loop 的 `anyhow`
  - 已完成：命令解析辅助、输入状态机等内部路径已统一收敛到 `HxResult`
  - 结果：当前源码中的 `anyhow` 已收敛到入口层

- [x] **[P0] 减少静默吞错：parse / render / inspector 刷新失败要可观测**
  - 已完成：inspector 初始化/刷新失败会在状态消息与 panel 中暴露，不再与“未识别到格式”混淆
  - 已完成：render 读数据失败会写入 stderr，并保留降级显示
  - 约束：后续不要再用 `unwrap_or_default()` 静默掩盖 parse / render / refresh 失败

- [x] **[P1] 拆分超大函数，降低认知负担**
  - 已完成：`handle_action` 已拆成 navigation / command / inspector / editor 分发
  - 已完成：`execute_command` 已按 quit/write/search/inspector/format 等类别拆成私有 handler
  - 已完成：`render_main` 已按“取数 / 组 line / 主 pane 绘制 / inspector 绘制”分段
  - 验证：阶段性回归与最终 `cargo fmt --check && cargo test --all-targets && cargo clippy --all-targets` 已通过

- [ ] **[P1] 拆分超大模块，避免“单文件状态机”继续膨胀**
  - 现状：`document.rs`、`state.rs`、`events.rs` 文件体积都偏大
  - 风险：职责边界变弱，后续改动更容易牵一发而动全身
  - 修改建议：
    - `document.rs` 可拆成 read / edit / search / save-facing helper
    - `state.rs` 可拆 selection、insert/edit、inspector state
    - `events.rs` 可拆 keyboard 与 mouse handler

- [ ] **[P1] 清理死代码、陈旧代码和半废弃 helper**
  - 现状：存在未使用模块或几乎只剩测试引用的函数
  - 风险：误导阅读者，增加维护噪音
  - 修改建议：
    - 已完成：删除未使用的 `src/core/search.rs`
    - 清理只剩测试使用、但已不符合主流程语义的 helper
    - 每次大改后跑一次引用搜索，避免遗留孤岛代码

- [ ] **[P1] 去重重复实现，避免 parser / hint / input 行为漂移**
  - 现状：如 `split_command()` 已有重复实现；部分 keymap 逻辑也平行维护
  - 风险：一个地方修改后，另一个地方忘记同步
  - 修改建议：
    - 已完成：命令切分逻辑已提成共享 helper
    - 共享 normal / visual 的通用导航键映射
    - 减少“逻辑一致但实现分散”的小工具函数

- [ ] **[P1] 收敛返回类型设计，去掉不必要的 `Result` 包装**
  - 现状：有些函数理论上很少失败，但返回 `Result`
  - 风险：调用栈看起来比实际复杂，API 表意弱
  - 修改建议：
    - 对纯状态切换类函数，优先返回 `()`
    - 只有真实可能失败、且调用者需要处理时才返回 `HxResult`
    - 重构时同步调整调用点，不要只改签名不改语义

- [ ] **[P2] 分层推进 pedantic 级别 lint 收敛**
  - 现状：pedantic 下 warning 数量较多
  - 风险：一次性整改成本高，容易把“风格整改”变成“逻辑大重写”
  - 修改建议：
    - 先处理 API 设计类 warning
    - 再处理格式化、`map_or`、`is_some_and` 之类机械型问题
    - 对暂不处理项记录 allow 或 issue，不要长期悬空

- [ ] **[P2] 统一文档注释与用户文档的一致性检查**
  - 现状：实现、README、测试、issue 有过漂移历史
  - 风险：后续开发者会被旧文档误导
  - 修改建议：
    - 用户可见行为改动时同步检查 `README.md`、提示文案、测试
    - 对核心语义变更补充 rustdoc 或开发文档说明
    - 把“文档同步”作为提交前 checklist 的固定项
