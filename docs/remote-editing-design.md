# Remote Editing Design

本文档规划 hxedit 的 remote 文件编辑能力。目标是支持远程大文件的 byte 级编辑，同时继续保护现有
`Document` 数据模型、undo / save / search 语义和大文件性能。

当前状态：已落地 `--remote sftp://...`（需 `remote-sftp` feature）、`FileView`
随机读 source 抽象、fake remote 测试后端、OpenSSH SFTP transport、可选 libssh2
SFTP 后端、远程 rewrite-save、保存前后 fingerprint 冲突检测，以及 App / headless
共享执行层接入。默认 OpenSSH transport 复用系统 SSH config、host-key 检查、公钥、
agent 和 GSSAPI 登录；`HXEDIT_SFTP_BACKEND=ssh2` 可强制走旧 libssh2 后端。仍未做
remote other-side `:diff`、远程 save-as、后台预取和更多协议。

---

## 1. 目标与非目标

### 目标

- 通过远程文件 target 打开、浏览、搜索、编辑、hash、export、save。
- 远程原始字节作为 `Document` 的 original source；overwrite / insert / tombstone delete /
  real delete 仍完全由现有 piece table、tombstone、replacement 模型表达。
- 读取必须支持按 offset 分块，继续复用 page cache 与 document walker，不能把远程文件默认整文件下载到内存。
- `:w` 对远程目标执行 rewrite-save，并在成功后 reload、清空 tombstone / replacement / undo / redo，
  与本地 save 的后置状态一致。
- headless macro / script / `--command` 与 TUI 共享同一个 remote document source，避免 TUI-only 功能分叉。

### 非目标

- 不做协同编辑、实时同步或远程文件 watch。
- 不做 patch-save fast path。远程首版仍沿用 rewrite-save，只有证明收益和正确性后再评估 patch-save。
- 不把 remote diff、远程目录浏览、远程进程内存编辑放进首版。
- 不在 TUI raw mode 内实现密码交互。认证应依赖 ssh agent / 系统 SSH 配置，或在进入 TUI 前失败并给出清晰错误。

---

## 2. 用户面草案

首版建议使用显式 remote target，避免和本地路径、Windows drive、`host:path` 形式混淆：

```bash
hxedit --remote sftp://user@host:22/absolute/path/to/file.bin
hxedit --readonly --remote sftp://host/var/log/blob.bin
hxedit --remote sftp://host/tmp/sample.bin --command "s x/de ad be ef/" --command "w"
```

约定：

- `--remote <uri>` 与 positional `FILE`、`--pid`、`--process` 互斥。
- 首版只接受 URI 形式，不接受 scp-like `host:path`。
- `:w` 保存回同一个远程文件。
- 远程文档上的 `:w <path>` 首版先拒绝，避免用户不清楚这是本地 save-as 还是远程 save-as。
  本地导出继续使用 `:export <path>`。
- `:diff <path>` 首版仍只比较本地 other file；remote other source 等主 document remote 语义稳定后再扩展。
- 状态栏和主视图路径展示使用 redacted remote label，不显示 URI 中可能出现的 secret。

---

## 3. 核心架构

### 3.1 抽象 remote 前先抽象 byte source

当前 `Document` 直接持有 `FileView`，`FileView` 只有 `Disk(File)` 和 `Memory(Vec<u8>)`。remote 支持应先收敛成一个随机读 source 抽象：

```rust
trait ByteSource {
    fn label(&self) -> SourceLabel;
    fn len(&mut self) -> HxResult<u64>;
    fn readonly(&self) -> bool;
    fn read_range(&mut self, offset: u64, len: usize) -> HxResult<Vec<u8>>;
    fn fingerprint(&mut self) -> HxResult<Option<SourceFingerprint>>;
    fn reload(&mut self) -> HxResult<()>;
}
```

实现分层建议：

- 保留 `FileView` 作为 page-cache 门面，先把内部 storage 从 `FileStorage` 扩成 `Box<dyn ByteSource>` 或
  小枚举 `LocalDisk / Memory / Remote`。
- page cache 只依赖 `read_range(offset, len)`，不再直接依赖 `std::fs::File + Seek`。
- `Document` 继续只通过 `view.read_range()` 和 walker 消费 original bytes。
- 不在 App、exec、format、diff 中直接调用 remote backend。

这样第一阶段可以只做本地行为等价重构，所有现有 tests / benches 都应该保持通过。

### 3.2 Document target

`Document` 需要从 `PathBuf` 扩到稳定的 target 描述：

```rust
enum DocumentTarget {
    Local(PathBuf),
    Remote(RemoteTarget),
    Memory(PathBuf),
}
```

注意：

- `Document::path()` 当前被 render、save、测试、memory label 使用。迁移时不要在大范围调用方一次性改语义。
  可先新增 `Document::label()` / `Document::target()`，保留 `path()` 给本地和兼容路径。
- remote label 必须可显示、可比较、可 redacted。
- memory 的 fixed-size 语义不能套到 remote。remote 文件允许 insert / tombstone / real delete 后 save 成不同长度。

### 3.3 Transport backend

首版推荐只支持 SFTP 语义，因为它天然支持 offset read/write/stat/rename，符合 hxedit 的分页模型。

需要在 prototype 阶段确认两件事：

- 默认使用外部 OpenSSH SFTP subsystem；libssh2 crate backend 作为显式 fallback。
- remote feature 是否进入 default bundle。

约束：

- `core` build 不应被 remote 依赖拖入额外网络或 C 依赖。
- OpenSSH transport 依赖运行时 `ssh` 可执行文件；错误信息要明确说明运行时依赖。
- crate backend 要同步 license / release notice，并评估 Windows / macOS / Linux 构建。
- 所有命令执行必须用 argv 传参，禁止 shell 拼接远程路径。

---

## 4. Save 语义

远程 save 必须保持与本地 rewrite-save 同类的后置不变量：

1. 用 `Document::walk_logical_chunks` 流式产生保存内容。
2. 写到远程同目录临时文件。
3. 尽量复制原文件权限 / owner 可见元数据；无法复制时给 warning，不伪装成功保留。
4. save 前重新 stat 原目标；若 open 时 fingerprint 存在且已经变化，拒绝保存并提示 remote changed。
5. 成功写完后尽量 fsync 远程临时文件；服务器不支持时降级为 warning。
6. 使用同目录 rename 替换目标。优先使用可覆盖且原子性更明确的扩展；不可用时根据 backend 能力选择拒绝或带 warning fallback。
7. reload remote source，重建 piece table，清空 tombstones / replacements / undo / redo，刷新 inspector。
8. 失败时 best-effort 删除临时文件；不得清空本地编辑状态。

首版不提供 remote `:w <path>`。后续如果要加 remote save-as，应引入显式 target：

```text
:w sftp://host/path/to/other.bin
```

本地副本输出继续使用 `:export <path>`，不把 export 和 save 混成同一个语义。

---

## 5. 冲突与一致性

远程文件可能在编辑期间被别人修改。首版应采用保守冲突策略：

- open 时记录 fingerprint：至少包含 len、mtime；如果 backend 能提供 inode/file-id/hash hint，也纳入。
- `:w` 前重新 fingerprint。
- fingerprint 变化时拒绝保存，提示用户远程文件已变化。
- 后续可以增加 force 语义，但必须经过命令设计，例如 `:w!` 或 `:remote save --force`。

不做的事：

- 不自动 merge。
- 不在每次读取前 stat。读取期间远端变化只在 save 前统一发现。
- 不因为远端变化把已有 edits 自动重放到新 base；这会改变 replacement / tombstone / insert 的锚点语义。

---

## 6. 受影响模块

| 模块 | 改动重点 |
|---|---|
| `src/cli.rs` | 增加 `--remote` target，保持与 file / pid / process 互斥 |
| `src/core/file_view.rs` | 抽象 page-cache 后端，保留本地行为等价 |
| `src/core/document/*` | target / label / save reload 迁移，不改变三类编辑语义 |
| `src/core/save.rs` | 拆出 local rewrite 与 remote rewrite sink，共享 logical chunk walker |
| `src/headless.rs` / `src/exec/session.rs` | 允许 remote target 走同一执行层 |
| `src/app.rs` / `src/app/commands/file_nav.rs` | 打开 remote document，`:w` 保存，远程 save-as 拒绝 |
| `src/view/status.rs` / `src/app/render/*` | 显示 redacted remote label、readonly / warning 状态 |
| `src/diff/*` | 首版不改 remote other；后续把 other side 也抽成 byte source |
| `docs/*` / `README*` / `commands/hints.rs` | 真正开放用户功能时同步用户文档与提示 |

---

## 7. 实施阶段

### Phase 0: 设计落地

- 保留本文档和 `docs/issues.md` backlog。
- 不改运行时代码。

### Phase 1: Source 抽象，无用户行为变化

- 把 `FileView` 的读后端抽象出来。
- 本地 disk / memory 行为保持等价。
- `Document::open`、save、hash、search、diff current-source 测试全部保持通过。
- 新增 fake source 测试 page cache 跨页、EOF、短读、错误传播。

### Phase 2: Remote target 骨架

- 增加 `RemoteTarget` parser 与 `CliTarget::Remote`。
- 增加 feature gate 和清晰的 feature-missing 错误。
- 用 fake remote backend 跑 TUI/headless 打开、读取、readonly、save 冲突测试。
- 不接真实网络。

### Phase 3: 真实 SFTP 读取

- 接入一个真实 SFTP backend。
- 支持 stat、len、readonly 检测、offset read。
- 验证大文件打开不整文件下载；render/search/hash 仍走分块读。
- 认证失败必须在进入 TUI raw mode 前报错。

### Phase 4: 远程 rewrite-save

- 实现远程临时文件写入、rename、reload。
- 加 conflict detection。
- App 和 headless 都支持 `:w` / `--command w`。
- 远程 `:w <path>` 明确拒绝并建议 `:export`。

### Phase 5: 用户文档和可选增强

- 同步 README / README_CN / user guide / hints / tests。
- 评估是否把 `remote` feature 纳入 default bundle。
- 增加 `:remote info` / `:remote reconnect` 等诊断命令。

### Later

- remote other side for `:diff`。
- 背景预取与延迟指标。
- 可取消的远程长操作进度。
- remote save-as。
- 基于 server 能力的 fsync / posix rename 能力展示。

---

## 8. 测试矩阵

默认 `cargo test` 不能依赖真实网络。需要覆盖：

- fake source: page cache 命中 / miss / EOF / 短读 / 错误传播。
- document invariants: overwrite、insert、tombstone delete、real delete、mixed edits 后 save 到 fake remote。
- save conflict: open 后 fingerprint 变化，`:w` 拒绝且 dirty / undo 保留。
- save failure: 写临时文件或 rename 失败，dirty / undo 保留。
- headless: `--remote fake://... --command ... --command w` 走同一 exec 层。
- App: remote readonly status、`:w` 成功清 dirty、`:w <path>` 拒绝。
- render: remote label redaction。

真实 SFTP 集成测试只在显式环境变量下运行，例如：

```bash
HXEDIT_REMOTE_TEST_SFTP=sftp://user@host/tmp/hxedit-test.bin cargo test remote_sftp -- --ignored
```

性能观测放到 bench 或手工 profile，不给 `cargo test` 加 wall-clock 阈值。

---

## 9. 风险与待决策

- Transport：默认 OpenSSH 兼容用户配置和 GSSAPI，但有运行时 `ssh` 依赖；crate backend
  更易封装但有 C 依赖、license 和跨平台维护成本。
- Rename 原子性：不同 SFTP server 能力不一致，必须显式建模 backend capability。
- 认证体验：不能让密码 prompt 出现在 TUI raw mode 中；OpenSSH transport 使用
  `BatchMode=yes`，crate backend 仍不支持 OpenSSH 的 GSSAPI-only 登录路径。
- Save-as 语义：远程文档上的 `:w <path>` 容易误解，首版先拒绝。
- 文件变化冲突：fingerprint 不是强一致锁，首版只做保守检测，不做锁定和 merge。
- 大延迟网络：remote page cache 至少使用 1 MiB 读取；clean remote 的 hash /
  search / binary export 使用 32 MiB streaming fast path，OpenSSH SFTP read request
  使用 256 KiB payload。dirty / overlay 区间仍回到普通 logical walker。后续仍可补
  后台进度和更细的自适应窗口。
