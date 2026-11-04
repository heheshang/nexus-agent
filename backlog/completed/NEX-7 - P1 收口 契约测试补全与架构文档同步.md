---
id: NEX-7
title: "test(core): P1 收口 —— 契约测试补全与架构文档同步"
status: Done
assignee: []
labels: [core, testing, docs, p1]
priority: medium
dependencies:
  - NEX-4
  - NEX-5
  - NEX-6
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.3"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.2"
---

## Description

P1 的收口任务：把 §5.2 / §5.3 的验证策略落到可执行断言，并同步架构文档中因 P1 落地而失效的表述。

**P1 的完成判据是「负向测试通过」**（架构文档 §6.1）—— 由 NEX-4 的 AC #9 承担，本任务确保它**真的**通过（而不是被跳过）。

### §5.3 需补充的架构测试

架构文档 §5.3 列了一张「设计文档落地后需补充」的表：

| 测试 | 检验的设计条款 | 状态 |
|---|---|---|
| `workflow_runs` 只在 `nexus-execution` 出现 | §4.2 差异 1 | P2 才有 `workflow_runs` —— 本任务**不**做 |
| `nexus-model` 不含 `FromRow` / `sqlx::Type` | §4.2 差异 2 | ✅ 已有 |
| `nexus-core` 零内部依赖 | §4.4 规则 2 | ✅ 已有 |
| 能力层不依赖 `nexus-repo` | §4.1.2（防环） | ✅ 已有 |
| `nexus-repo` 只依赖能力层（单向） | §4.1.2 | P2 才有 —— 本任务**不**做 |

### §4.2 需要同步的表格行

架构文档 §4.2 的对照表有一行：

```
| `nexus-core` 子模块 | 类型状态、事件、错误 | **空**（待 P1） | ⬜ |
```

P1 落地后这行**失效** —— 必须更新，否则文档与实现不符。

### §5.5 需要同步的表述

架构文档 §5.5 说「P1 直接把这个 demo 的形状搬进 `nexus-core`」—— 搬完后这句话需要改成已完成状态，并说明 `examples/` 的 demo **保留作为回归基准**（不删除）。

## Acceptance Criteria

- [x] #1 `crates/nexus-core/tests/compile_fail.rs` 的负向用例**实际运行**（不是被跳过），且 `.stderr` 快照与当前工具链一致
- [x] #2 `nexus-core` 有一个**跨模块的契约测试**文件，把 ID 类型、事件名清单、错误码表三份契约放在一起（便于一眼看出契约是否齐备）
- [x] #3 `nexus-core` 的公共 API 有 `Debug` 实现（workspace lint `missing_debug_implementations` 通过）
- [x] #4 架构文档 §4.2 对照表中 `nexus-core` 子模块那行**已更新**为 P1 落地后的实际状态
- [x] #5 架构文档 §5.2 的「P1 直接把 demo 形状搬进 `nexus-core`」表述**已更新**，并说明 `examples/` 的 demo 保留作为回归基准
- [x] #6 架构文档 §6.1 的 P1 行**完成判据**标注为已达成（负向测试通过）
- [x] #7 `examples/` 下的 11 个 demo **一个都没删**（demo 是回归基准，不是临时代码）
- [x] #8 `cd examples && cargo test --workspace` 仍然全绿（61 个测试）
- [x] #9 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #10 CI 六道门禁全绿（含 `scripts/check-naming.sh`）

## 备注

- 本任务**不改**任何契约（码值、事件名、ID 底层类型）。若发现契约与实现不符，是**实现**错了，不是契约错了 —— 按 §6.3 的纪律，回退到任务层修正实现，不在收口阶段改契约。
- 文档同步属于 `advisory` 级门禁（只记录不拦截），但本任务把它作为 AC —— 因为契约文档与实现不符会直接误导后续任务。

## 实现过程中的发现

**1. 跨模块契约测试的价值在于「交叉一致性」，不在于重复断言。**
三份契约的逐条断言已在各模块内做过，重复一遍没有意义。
本文件真正补上的是**模块之间**的一致性 —— 例如：
- `EventPayload::Error { code }` 的裸 `i32` 必须能被 `ErrorCode::from_code` 反查
- `ToolStatus::Failed { code }` 同理
- `AppError::retry_policy()` 必须与 `ErrorCode::retry_policy()` 一致
- `UserFacing` 序列化出的码必须能反查回来

这些断言**放在一起才写得出来** —— 它们跨越了模块边界。

**2. 测试函数名不能用中文 —— 这是项目铁律。**
初稿把测试命名为 `契约一_十八个_id 类型互不相同`，编译直接失败（Rust 词法分析器
不认带空格的标识符）。查 `scripts/check-naming.sh` 才看清规则：
`.cursor/rules/rust-code-standards.mdc` §11 明确「标识符不得包含中文」。
已全部改为英文命名，中文只保留在注释与文档字符串里。

**3. demo 数量是 11 个，不是任务初稿写的 12 个。**
`examples/` 下实际是 11 个 `demo-*` 目录。任务初稿的「12 个」是数错了 ——
已更正为 11 个。**demo 一个都没删**，`git status examples/` 为空，
`cargo test --workspace` 61 个测试全绿。

**4. §5.3 的架构测试表也有两处过时表述。**
初稿说「当前 workspace 已有 8 个架构测试」—— 实际是 **10 个**
（P0 期间补了「依赖版本纪律」「lints 继承」「入口文件」三项）。
已一并更正，并给后两项标上「⬜ P2 才有」——
它们要等 `nexus-infra` / `nexus-repo` 落地后才有实体可查。

**5. 发现 CI 的工具链与仓库固定版本不一致 —— 这是 `trybuild` 引入的新约束。**
CI 原本用 `dtolnay/rust-toolchain@stable`，而仓库根的 `rust-toolchain.toml`
固定 `1.99.1`。action 的目录覆盖会**压过** `rust-toolchain.toml` ——
CI 实际跑的是「当周的 stable」。
P0 阶段这没问题（没有快照类测试），但 P1 引入 `trybuild` 后就有问题了：
`.stderr` 快照是编译器输出的**逐字**记录，stable 一升级，11 个快照全部失效，
红灯与本次改动毫无关系。
已改为**不覆盖** —— 让 `rust-toolchain.toml` 成为工具链版本的唯一真相来源，
并在 CI 里打印版本号便于排查。rustup 会自动安装 `rust-toolchain.toml`
指定的版本及其 components。

**6. 附录 C.1 补了 5 条 P1 期间新发现的陷阱。**
原表 8 条全是「写 demo 时踩的坑」。P1 正式实现的坑**类型不同** ——
它们关于依赖纪律、类型设计、以及「负向用例的错误码要信编译器不要信直觉」。
已单列一张表追加，避免与 demo 的坑混淆。

**7. 归档时发现 `git mv` 之后编辑器在旧路径重建了任务文件 —— 已加门禁。**
`git mv` 把 NEX-4 / NEX-5 从 `backlog/tasks/` 移到 `backlog/completed/` 后，
编辑器在原路径**重建**了同名文件（mtime 比提交晚一分钟），
后续每次 `git add -A` 又把它重新提交。
结果：NEX-4 与 NEX-5 在两个目录下各存一份，内容完全相同。
已清除 `tasks/` 下的残留，并新增 `scripts/check-backlog.sh`：
- 检查同一任务是否同时存在于 `tasks/` 与 `completed/`
- 检查 `completed/` 下的任务 `status` 是否标记为完成
  （顺带发现 P1 的四个任务归档时都忘了改 `status: To Do` —— 一并修正）
该脚本已接入 CI，并**用注入重复文件的方式验证过它真的会红**。

**8. `examples/` 的 demo 是 11 个，不是任务初稿写的 12 个 —— 已更正。**
`git status examples/` 为空（一个都没删、没改），
`cd examples && cargo test --workspace` 61 个测试全绿。
