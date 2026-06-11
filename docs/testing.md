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
- 当前性能观测入口是 `cargo bench --bench perf_bench`，覆盖 16MB/64MB save、hash、logical_bytes、paste overwrite、piece lookup、ELF parse、当前 search；`cargo test` 里的大文件用例只保留 correctness 断言，不做 wall-clock 阈值
- `src/app/tests.rs` 已接入测试编译；新增 App 层测试时，优先放到真正会执行的位置
- 如果你改了 inspector / parse / render 相关路径，不要静默吞错；至少要在状态栏、panel 或 stderr 中留下可观测线索

---

## 2. 文档同步要求

如果你的改动影响用户可见行为，必须同步检查：

- `README.md` / `README_CN.md`
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

若改动 license / feature bundle / release 打包，必须同步 `Cargo.toml`、`LICENSE*`、`licenses/THIRD_PARTY_NOTICES.txt` 与 `licenses/keystone/FOSS-NOTICE.txt` 与 release workflow；`full` 产物必须继续附带 Keystone 的 FOSS notice / license / exception 文件，且不要把 `full` 写成 `MIT/Apache-only`。
