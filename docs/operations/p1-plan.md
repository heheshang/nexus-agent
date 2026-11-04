# P1 · 核心类型

架构文档 §6.1 的 P1 阶段：**`nexus-core` 的类型 —— 取消语义、事件、错误**。

## 任务拆解

| 任务 | 内容 | 依赖 | 对应设计条款 |
|---|---|---|---|
| **NEX-4** | 类型化 ID（18 种）+ 取消语义类型状态 | — | §2.1、§2.8.1 |
| **NEX-5** | 事件负载（12 个）+ 单写者事件流 | NEX-4 | §2.2、§2.2.4.1 |
| **NEX-6** | 三层错误模型 + 42 个业务码契约 | NEX-4 | §2.6、§2.6.4.1 |
| **NEX-7** | P1 收口：契约测试补全 + 架构文档同步 | NEX-4/5/6 | §5.2、§5.3、§4.2 |

依赖图：

```
NEX-4 (ID + 取消语义)
  ├── NEX-5 (事件)        ← 需要 RunId
  └── NEX-6 (错误)        ← 需要 ErrorCode 相关 ID
        └── NEX-7 (收口)  ← 需要三者全部
```

## 完成判据

架构文档 §6.1：**「负向测试通过」** —— 具体由 NEX-4 的 AC #9（`trybuild` 断言 `DetachedScope` 上调 `cancel()` 编译失败）承担，NEX-7 负责确认它**真的运行**而非被跳过。

## 三条不可让步的红线

### 1. 契约不可改

ID 底层类型、事件名与字段名、错误码值 —— 这三样是**前后端契约**（§6.3 已定稿）。

实现与契约不符时，**是实现错了，不是契约错了**。回退到任务层修正实现，不在收口阶段改契约。

### 2. 类型状态不能退化成布尔标志

§2.1.2 明确反对 `cancelable: bool` 与 token 并存 —— 那是「两个独立的真相来源」，写错一个另一个不报错。

`DetachedScope` 上**不存在** `cancel()` 是核心主张，必须由 `trybuild` 负向测试固化，`.stderr` 快照入库。

### 3. `UnknownOutcome` 不能并入 `Retryable`

§2.6.3 是本设计相对参考实现的**关键改进**：参考实现的 `IsRetryable` 把 504（超时，可能已执行）与 502（明确拒绝）一视同仁。504 下重试支付类工具会重复扣费。

6 个 `UnknownOutcome` 码：`ModelTimeout` / `ToolFailed` / `ToolTimeout` / `SandboxTimeout` / `McpToolFailed` / `A2ATaskFailed`。

## 已知的设计空白

`RunScope` —— 架构文档 §2.1.6 提到它，但**未给形态**（未说明与 `RequestScope` 的关系）。

按 §6.3「未定稿的条款不允许在实现中顺手决定」，NEX-4 **只实现** `RequestScope` 与 `DetachedScope`。`RunScope` 留到有定稿时再做 —— 需要先补 RFC 或设计文档修订。

## 参考实现

`examples/` 下的 12 个 demo 是**设计验证的可执行部分**，P1 的形状来源：

| demo | 验证条款 | 搬到哪个任务 |
|---|---|---|
| `demo-cancel-typestate` | §2.1 类型状态 + trybuild | NEX-4 |
| `demo-event-payload` | §2.2.4 事件扁平化 | NEX-5 |
| `demo-error-model` | §2.6 三层错误模型 | NEX-6 |
| `demo-backpressure` | §3.2 双通道 | P2/P5 |
| `demo-canonical-json` | §3.5 规范化 JSON | P6 |
| `demo-sql-guard` | §3.6 AST 级 SQL 校验 | P2 |
| `demo-sse-frames` | SSE 帧边界 | P5 |
| `demo-rmcp-boundary` | §4.3.1 rmcp 能力边界 | P8 |
| `demo-consumer-trait` | §3.1 消费者定义 trait | P4/P2 |
| `demo-stream-fallback` | §3.3 流式不可回退 | P3 |
| `demo-lease-fencing` | §2.5.4 SKIP LOCKED + fencing | P6 |

**demo 保留，不删除** —— 它们是回归基准（NEX-7 AC #7）。

注意：demo 的形状**不能直接搬**。例如 `demo-cancel-typestate` 用 `Arc<dyn Fn()>` 回调模拟取消，而 §2.1.5 定稿的是 `tokio_util::sync::CancellationToken`。
