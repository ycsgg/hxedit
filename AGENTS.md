# AGENTS.md

本文件是后续在本仓库工作的开发者 / 代理的**入口**。它只保留最高层约束与索引，细节请按主题进入 `docs/`。

---

## 仓库目标

`hxedit` 是一个面向大文件的 TUI hex editor。优先保护以下能力，不要为了快速加功能破坏核心模型：

- 正确的 byte 级编辑语义
- 稳定的 undo / save / search
- 大文件可接受的性能
- 文档与实现一致

---

## 一条最重要的原则

**先保护数据模型和交互语义，再追求功能数量。**

改任何代码前，先确定你操作的是 **real delete / tombstone delete / replacement** 中的哪一类，不要“顺手写成另一种”。

---

## 文档索引

开始改代码前，先读 `README.md`，再按主题进入下列文档：

| 文档 | 内容 |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | 当前快照、产品定位、已落地用户面、行为边界、代码地图、CI/Release、仍需关注的点 |
| [`docs/editing-model.md`](docs/editing-model.md) | 三类编辑语义、长度 / offset、光标与 mode 规则、inspector / save 不变量 |
| [`docs/modules.md`](docs/modules.md) | 各高风险模块的改动须知（document / piece_table / app / commands / events / render / palette / format / memory） |
| [`docs/sagitta-integration-design.md`](docs/sagitta-integration-design.md) | Sagitta 后台分析、symbol panel 覆盖、offset 有效性与失效规则 |
| [`docs/workflow.md`](docs/workflow.md) | 推荐工作方式、常见改动路径、提交前自检清单、优先级共识 |
| [`docs/testing.md`](docs/testing.md) | 测试要求与文档同步要求 |
| [`docs/issues.md`](docs/issues.md) | 仍有行动价值的 backlog |
| [`docs/archive/`](docs/archive/) | 已完成 / 过时事项归档 |

---

## 文档维护约定

- 改动**用户可见行为**时，同步 `README.md` / `README_CN.md` / `docs/issues.md` / 命令提示（`commands/hints.rs`）/ 对应测试。
- 改动**内部约束或工作流**时，同步本文件与 `docs/` 下对应主题文件。
- 这些文档保持**短、当前、可执行**：已完成或过期的长列表移到 `docs/archive/`，不要继续占容量。
- **本文件与 `docs/` 下文档随 git 一起提交，纳入版本管理。**
