# Testing & Documentation Sync

---

## 1. 测试要求

`.github/workflows/ci-release.yml` 当前把 Rust 固定到 `1.94.1`；GitHub Actions 在 Ubuntu / Windows 执行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --all-targets`，并覆盖 `core` / `default` / `full` 三个 feature 档位；发布 job 当前构建 `OS * arch * feature` 矩阵产物。

默认最低要求：

```bash
cargo fmt --check
cargo test --all-targets
```

改动较大时再跑：

```bash
cargo clippy --all-targets
```

注意：

- 默认级别 `cargo clippy --all-targets` 应保持无 warning；更严格的 pedantic 级别可分批推进
- 性能类 benchmark 若继续增长，优先迁移到 `cargo bench`，不要长期混在 `cargo test` 主路径里拖慢 correctness 回归
- 当前性能观测入口是 `cargo bench --bench perf_bench`，每个 bench 以隔离子进程运行并输出用时与峰值 RSS；默认矩阵只跑当前产品路径上的场景，覆盖 16MB/64MB save、hash、logical_bytes、paste overwrite、piece lookup、ELF parse、search、diff，以及 replacement / real insert / tombstone delete / real delete 等编辑模式指令。新增的连续/复杂场景可用 `HXEDIT_BENCH_FILTER=session_256mb_mixed_10k_ops`、`HXEDIT_BENCH_FILTER=undo_redo`、`HXEDIT_BENCH_FILTER=dirty_islands`、`HXEDIT_BENCH_FILTER=search_256mb_dirty_many_islands` 单独跑；对外展示子集可用 `HXEDIT_BENCH_SUITE=public cargo bench --bench perf_bench`，重复测量可加 `HXEDIT_BENCH_REPEAT=<N>`；最多 1GiB 的大文件观测需要显式设置 `HXEDIT_BENCH_LARGE=1`，当前只覆盖 search / hash / diff 等不会依赖系统剪贴板容量的流式路径；历史对照 bench 需要显式设置 `HXEDIT_BENCH_LEGACY=1`，例如 `HXEDIT_BENCH_LEGACY=1 HXEDIT_BENCH_FILTER=per_byte cargo bench --bench perf_bench`；`cargo test` 里的大文件用例只保留 correctness 断言，不做 wall-clock 阈值
- 固定 seed fuzz 入口是 `cargo test --test insert_mode deterministic_mixed_document_edit_fuzz_matches_reference_model`。新增 fuzz 操作时必须同步更新 `ReferenceDoc` 语义、错误路径断言和每步后的 invariant 检查；详细规则见 `docs/workflow.md`
- `src/app/tests.rs` 已接入测试编译；新增 App 层测试时，优先放到真正会执行的位置
- 如果你改了 inspector / parse / render 相关路径，不要静默吞错；至少要在状态栏、panel 或 stderr 中留下可观测线索

---

## 2. 文档同步要求

如果你的改动影响用户可见行为，必须同步检查：

- `README.md` / `README_CN.md`
- `docs/user-guide.md` / `docs/user-guide_CN.md`
- `docs/issues.md`
- 命令提示 / 帮助文本（`commands/hints.rs`）
- 对应测试

仓库当前有“README 与实现漂移”的历史问题，不要继续扩大。

如果改动影响内部约束或工作流，记得同步：

- `AGENTS.md`（入口）
- `docs/` 下对应主题文件（architecture / editing-model / modules / workflow）
- `docs/issues.md`

这些内部文档应该保持**短、当前、可执行**：

- `docs/architecture.md` 只保留当前架构 / 不变量 / 推荐路径
- `docs/issues.md` 只保留仍有行动价值的 backlog
- 已完成或已过期的长列表移到 `docs/archive/`，不要继续占容量

若改动 license / feature bundle / release 打包，必须同步 `Cargo.toml`、`LICENSE*`、`licenses/THIRD_PARTY_NOTICES.txt` 与 `licenses/keystone/FOSS-NOTICE.txt` 与 release workflow；`full` 产物必须继续附带 Keystone 的 FOSS notice / license / exception 文件，并保留 Sagitta notice，且不要把 `full` 写成 `MIT/Apache-only`。
