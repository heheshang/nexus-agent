---
id: NEX-20
title: "docs(llm): P3 收口 —— 架构文档同步与熔断粒度声明"
status: Todo
assignee: []
labels: [llm, docs, p3]
priority: medium
dependencies:
  - NEX-16
  - NEX-17
  - NEX-18
  - NEX-19
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.3"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.4"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1"
  - "docs/operations/p3-plan.md"
---

## Description

P3 的收口任务：**「预算裁剪与回退边界有测试」**两项判据的最终确认，
外加同步架构文档中因 P3 落地而失效的表述。

### §5.4 的不变量表 —— 新增 P3 已验证项

| 不变量 | 测试方式 | 状态 |
|---|---|---|
| 预算不足在**调用前**被拒 | 单元测试：超预算 → `BudgetExceeded`，且无上游请求 | 待补 |
| 预算错误**不可重试** | 断言 `RetryPolicy::Terminal` | 待补 |
| 预算裁剪是**纯函数** | 同输入同输出，入参未变 | 待补 |
| 流式首事件**前**可回退 | 网关测试 | 待补 |
| 流式首事件**后**不可回退 | 网关测试 | 待补 |
| fallback 与 primary 同供应商**不切换** | 网关测试 | 待补 |
| `UnknownOutcome` **不重试** | 重试测试 | 待补 |
| 熔断**预热期**不评估错误率 | 熔断器测试 | 待补 |
| 取消**不计入**熔断样本 | 熔断器测试 | 待补 |
| 每次尝试（含 fallback）**重跑**预算裁剪 | 网关测试 | 待补 |

### §5.3 的架构测试表 —— 新增 P3 相关项

| 测试 | 检验的设计条款 |
|---|---|
| `nexus-llm` 不含 `Box<dyn` （enum 优于 dyn） | §2.3.2 |
| `nexus-llm` 不依赖 `nexus-repo`（不查库） | 参考架构 §0.3 |
| `nexus-llm` 的熔断器用 `std::sync::Mutex` 而非 `tokio::sync::Mutex` | 参考架构 §3.1.3 |

### §6.1 的 P3 行

标注完成判据已达成。

### 熔断粒度的**显式声明**

参考架构 §5.3 局限 #3 要求 Rust 版**表态**：
熔断是进程内状态，多副本部署需外置，**或在文档中显式声明「熔断是单副本粒度」**。

本阶段选了后者 —— 必须在架构文档里**明确写出**，不能让读者以为多副本下也一致。

### §4.3 的表格行

`nexus-model` / `nexus-llm` 相关行若因 P3 落地而变化，需同步。

## Acceptance Criteria

- [ ] #1 架构文档 §5.4 不变量表**已补** P3 已验证项（逐条对应上表的 10 条）
- [ ] #2 架构文档 §5.3 架构测试表**已补** P3 的三项结构层测试
- [ ] #3 架构文档 §6.1 的 P3 行**已标记**为完成，并写明完成判据的达成方式
- [ ] #4 架构文档**明确声明**「熔断是单副本粒度」—— 写进 §4.3 或新增说明小节
- [ ] #5 `docs/operations/p3-plan.md` **已更新**为完成状态，并记录实际交付
- [ ] #6 `nexus-llm` 的 crate 级文档写明「不查库」「熔断单副本粒度」「只做 OpenAI 兼容协议」
- [ ] #7 架构文档 §2.6.4.1 的业务码表**未被改动**（P3 不新增码值）—— 有测试或 diff 证据
- [ ] #8 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #9 CI 六道门禁全绿

## 备注

- 收口任务的**价值在「核对判据是否真的成立」**，不是「重做一遍」。
  逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。
- 「P3 完成」**不等于**「模型网关完备」——
  真实上游（真实模型服务）的连通性、成本核算的准确性、跨副本的熔断一致性
  都**不在** P3 范围内。这些要在文档里写清楚，避免读者误读「完成」。
- 附录 C 的「尚未验证的设计条款」表里若有 P3 相关项（如 §2.3.2 enum 优于 dyn 的性能收益），
  按实际验证情况更新 —— **没验证的不要标成已验证**。

## 实现过程中的发现

（待补）
