# examples · 设计验证 demo

> 定位：验证**设计文档**里尚未证实的条款。**不是**产品代码，也不是玩具示例。
>
> 设计文档：`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`
> 附录 C 列出了「尚未验证的设计条款」—— 本目录里的每个 demo 对应其中一条或多条。
>
> ## 为什么单独一个 workspace
>
> 这些 demo 有意使用**与生产代码不同的依赖组合**（例如 `rmcp` 需要开启
> `transport-streamable-http-server` 等 feature，而主 workspace 的 `rmcp`
> 只开了子集）。把它们放进主 workspace 会：
>
> 1. 迫使主 workspace 承担 demo 的依赖 —— 污染 `Cargo.lock`，放大 CI 时间
> 2. 让 `cargo clippy --workspace` 把 demo 的代码也算进产品 lint
> 3. 让架构测试（扫描 `crates/`）误判
>
> 所以本目录是**独立 workspace**，有自己的 `Cargo.lock`，用 `exclude` 与主 workspace 隔离。
>
> ## 运行
>
> ```bash
> cd examples
> cargo run -p demo-cancel-typestate      # §2.1 取消语义的类型状态
> cargo run -p demo-event-payload         # §2.2 事件负载的序列化形状
> cargo run -p demo-error-model            # §2.6 错误模型的三层分离
> cargo run -p demo-sse-frames            # §3.2 SSE 帧边界
> cargo run -p demo-backpressure          # §3.2 可丢/必达双通道
> cargo run -p demo-canonical-json        # §3.5 规范化 JSON
> cargo run -p demo-sql-guard             # §3.6 SQL AST 级校验
> cargo run -p demo-rmcp-boundary         # 待定 1 · rmcp 能力边界
> ```
>
> 需要数据库的 demo：
>
> ```bash
> cd examples
> DATABASE_URL=postgres://nexus:nexus@127.0.0.1:5432/nexus_design \
>   cargo run -p demo-lease-fencing
> ```
>
> ## 状态
>
> | demo | 验证的设计条款 | 状态 |
> |---|---|---|
> | `demo-cancel-typestate` | §2.1.3 类型状态禁止取消 | `trybuild` 负向测试已固化 |
> | `demo-event-payload` | §2.2.4 事件负载扁平化 | ✅ |
> | `demo-error-model` | §2.6.5 内部原因不外泄 | ✅ |
> | `demo-backpressure` | §3.2 双通道背压 | ✅ |
> | `demo-canonical-json` | §3.5 规范化稳定性 | ✅ |
> | `demo-sql-guard` | §3.6 AST 级 SQL 校验 | ✅ 24 用例 |
> | `demo-sse-frames` | SSE 帧边界（半帧缓冲） | ✅ 含死循环回归测试 |
> | `demo-rmcp-boundary` | §4.3.1 rmcp 客户端/服务端能力 | ✅ |
> | `demo-consumer-trait` | §3.1 消费者定义 trait 打断环 | ✅ |
> | `demo-stream-fallback` | §3.3 流式开始后不可回退 | ✅ |
> | `demo-lease-fencing` | §2.5.4 `SKIP LOCKED` + fencing | ✅（需 PG） |
>
> 共 **61 个测试**。
>
> 设计文档附录 C.1 记录了写这些 demo 时踩到的 **8 个实现陷阱**
> （SSE 分隔符死循环、`serde(flatten)` 与 enum tag 不兼容、`ErrorCode` 序列化形状等）——
> 正式实现时直接搬对应的回归测试过去。
