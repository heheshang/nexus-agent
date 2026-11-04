//! 事务 outbox 的**类型**部分。
//!
//! # 机制（参考架构文档 §4.1.1、§4.1.2）
//!
//! 事件与状态在**同一个事务**里写入 `execution_outbox` ——
//! 这样「状态变了但事件丢了」与「事件发了但状态没变」都不可能发生。
//!
//! ```text
//! 同一个事务：
//!   1. UPDATE execution_runs SET next_seq = next_seq + 1 ... RETURNING next_seq
//!   2. INSERT execution_events (run_id, seq, ...)
//!   3. INSERT execution_outbox (run_id, seq, topic, payload)
//! ```
//!
//! # 投递语义是**至少一次**，不是恰好一次
//!
//! 参考实现 `outbox.go:28-29` 的注释原文：
//!
//! > Delivery is at least once: a crash after the peer accepts but before the PG
//! > commit can redeliver the same (run_id,seq). Consumers must deduplicate by it.
//!
//! **「至少一次」是语义保证，不是 bug。**
//! 消费方**必须**按 `(run_id, seq)` 去重 —— 这是消费方的义务，不是投递方的。
//!
//! 这条不写在类型里就会被忽略，所以它写在 [`OutboxMessage`] 的文档注释里 ——
//! 消费方拿到这个类型时**不可能**看不到。
//!
//! # 为什么不去重成「恰好一次」
//!
//! 「恰好一次」需要**分布式事务**或**两阶段提交** ——
//! 那是把一个数据层的保证升级成跨系统的协调问题。
//! 参考实现明确选了「至少一次 + 显式去重元组」，本设计照抄。
//!
//! 去重元组是 `(run_id, seq)` —— 数据库对它建了唯一索引
//! （`execution_outbox_run_id_seq_key`），所以「同一 run 的同一序号」
//! 在 outbox 里**物理上不可能**出现两次。

use nexus_core::id::RunId;
use serde_json::Value;

/// 幂等键的**最大长度**。
///
/// 数据库列是 `VARCHAR(64)` —— 但应用层**也要**校验，
/// 给出清晰的业务错误，而不是把 PG 的错误往上冒。
///
/// # 为什么应用层要重复校验
///
/// 数据库的 `VARCHAR(64)` 拒绝时抛的是**约束冲突**，
/// 映射成 `InvalidParam` 是对的，但错误消息里带的是表名与约束名 ——
/// 那是内部结构，不该出现在对外响应里。
///
/// 应用层校验能给出**面向调用方**的消息：「幂等键超长」，
/// 而 DB 错误只能进 `cause`。
pub const MAX_REQUEST_KEY_LEN: usize = 64;

/// 幂等键的**最小长度**。
///
/// 1 —— 空字符串不是合法的幂等键：
/// 空键会让**所有**空键的提交撞进同一个 `(owner_id, '')` 桶里，
/// 那不是幂等，那是把所有请求混成一条。
pub const MIN_REQUEST_KEY_LEN: usize = 1;

/// outbox 消息。
///
/// # 消费方**必须**按 `(run_id, seq)` 去重
///
/// 投递是**至少一次**的：投递方在「对端已接受」与「PG 提交 `published_at`」
/// 之间崩溃，重启后会**重投同一条**。
///
/// 消费方**必须**按 `(run_id, seq)` 去重 —— 重复投递是**契约的一部分**，
/// 不是异常状况。
///
/// # 为什么 `(run_id, seq)` 是可靠的去重键
///
/// - `run_id` 定位是哪一次运行
/// - `seq` 是该运行的**单调递增**序号 —— 同一 run 内不会重复
/// - 数据库对它建了唯一索引 —— 同一 `(run_id, seq)` 在 outbox 里**只有一条**
///
/// 三者合起来构成**全局唯一**的标识。
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxMessage {
    /// outbox 行 ID。
    ///
    /// 只用于**投递方**标记 `published_at` —— 消费方用 `(run_id, seq)`。
    pub id: i64,
    /// 所属运行。
    pub run_id: Option<RunId>,
    /// 事件序号 —— 去重元组的第二半。
    ///
    /// `None` 表示这条 outbox 消息**不**对应某个运行的事件
    /// （`run_id` / `seq` 在表里都可空）。
    pub seq: Option<i64>,
    /// 主题 —— 消费方按它路由。
    pub topic: String,
    /// 负载。
    pub payload: Value,
    /// 去重键（`dedup_key` 列）。
    pub dedup_key: Option<String>,
}

impl OutboxMessage {
    /// 去重元组 —— 消费方去重的依据。
    ///
    /// # 返回 `None` 的含义
    ///
    /// 这条消息**没有** `(run_id, seq)` —— 它不是某次运行的事件
    /// （`run_id` 或 `seq` 为空）。
    ///
    /// 消费方遇到 `None` 时**不能**跳过 —— 那会丢消息。
    /// 此时应退回用 `dedup_key` 去重，或按 `id` 去重
    /// （`id` 是 outbox 表的自增主键，**每条消息一个**）。
    ///
    /// # 为什么不能只暴露 `id`
    ///
    /// `id` 能唯一标识 outbox 里的**一行**，但它**不是**事件的标识 ——
    /// 消费方拿 `id` 去和上游事件比对时，对不上。
    /// 契约里的去重元组是 `(run_id, seq)`，`id` 只是投递方的内部句柄。
    pub fn dedup_tuple(&self) -> Option<(RunId, i64)> {
        match (self.run_id, self.seq) {
            (Some(run_id), Some(seq)) => Some((run_id, seq)),
            _ => None,
        }
    }

    /// 是否是「运行事件」类消息（有完整去重元组）。
    pub fn has_dedup_tuple(&self) -> bool {
        self.dedup_tuple().is_some()
    }
}

/// 幂等键。
///
/// # 为什么用类型而不是 `&str`
///
/// 幂等键有**两种**不同的失败：
///
/// - **超长 / 空** —— 调用方的**参数**错了，**永远不会**因为重试而变好
/// - **键冲突** —— 不是错误，是「你要的 run 已经存在了」
///
/// 把两者混在一个 `Result` 里，调用方就会用同一种方式处理，
/// 而它们的处置**完全不同**。
///
/// 所以校验用 [`RequestKey`] 类型承载 —— 不合法就**构造不出来**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestKey(String);

impl RequestKey {
    /// 校验并构造幂等键。
    ///
    /// # 错误
    ///
    /// - 长度为 0 → [`ErrorCode::InvalidParam`]（空键会让所有空键提交撞进同一个桶）
    /// - 长度 > 64 → [`ErrorCode::InvalidParam`]（与数据库 `VARCHAR(64)` 一致）
    ///
    /// # 为什么长度错误是 `InvalidParam` 而不是别的
    ///
    /// 它是**调用方给了非法参数** —— 终止型，重试没有意义。
    /// 数据库的约束冲突也会映射到同一个码，但消息更笼统。
    pub fn new(key: &str) -> Result<Self, nexus_core::error::AppError> {
        let len = key.chars().count();

        if len < MIN_REQUEST_KEY_LEN {
            return Err(nexus_core::error::AppError::new(
                nexus_core::error::ErrorCode::InvalidParam,
                "幂等键不能为空",
            )
            .with_cause_msg(format!(
                "幂等键长度必须 ≥ {MIN_REQUEST_KEY_LEN}，实际 {len}"
            )));
        }

        if len > MAX_REQUEST_KEY_LEN {
            return Err(nexus_core::error::AppError::new(
                nexus_core::error::ErrorCode::InvalidParam,
                "幂等键超长",
            )
            .with_cause_msg(format!(
                "幂等键长度必须 ≤ {MAX_REQUEST_KEY_LEN}，实际 {len}"
            )));
        }

        Ok(Self(key.to_owned()))
    }

    /// 幂等键的字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RequestKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 幂等提交的结果。
///
/// # 为什么是枚举而不是 `RunId`
///
/// 「提交成功」与「命中幂等键、返回既有 run」在**数据上**都是「得到了一个 run ID」，
/// 但调用方往往需要区分：
///
/// - 首次提交 → 可能要触发「开始执行」
/// - 幂等命中 → **不能**再次触发执行（那会跑两遍）
///
/// 所以这里区分两种结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// **首次**提交 —— 创建了新的 run。
    Created(RunId),
    /// 幂等键命中 —— 返回**既有**的 run，**没有**创建新的。
    ///
    /// # 这不是错误
    ///
    /// 「重复提交返回既有 run」**就是幂等的定义**。
    /// 调用方**不应该**把它当失败处理。
    Existing(RunId),
}

impl SubmitOutcome {
    /// 两种结果都指向一个 run —— 提取它。
    pub fn run_id(self) -> RunId {
        match self {
            Self::Created(id) | Self::Existing(id) => id,
        }
    }

    /// 是否是首次创建。
    pub fn is_created(self) -> bool {
        matches!(self, Self::Created(_))
    }

    /// 是否命中了幂等键。
    pub fn is_existing(self) -> bool {
        matches!(self, Self::Existing(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;

    #[test]
    fn request_key_accepts_a_normal_key() {
        let key = RequestKey::new("req-abc-123").expect("应通过校验");

        assert_eq!(key.as_str(), "req-abc-123");
    }

    #[test]
    fn request_key_accepts_exactly_the_maximum_length() {
        let key = "k".repeat(MAX_REQUEST_KEY_LEN);
        let parsed = RequestKey::new(&key).expect("边界长度应通过");

        assert_eq!(parsed.as_str().len(), MAX_REQUEST_KEY_LEN);
    }

    #[test]
    fn request_key_rejects_one_character_over_the_maximum() {
        let key = "k".repeat(MAX_REQUEST_KEY_LEN + 1);
        let err = RequestKey::new(&key).expect_err("超长应被拒绝");

        assert_eq!(err.user_facing().code, ErrorCode::InvalidParam);
        // 终止型 —— 重试不会让键变短。
        assert_eq!(err.retry_policy(), nexus_core::error::RetryPolicy::Terminal);
    }

    #[test]
    fn request_key_rejects_the_empty_key() {
        let err = RequestKey::new("").expect_err("空键应被拒绝");

        assert_eq!(err.user_facing().code, ErrorCode::InvalidParam);
    }

    #[test]
    fn request_key_length_is_counted_in_characters_not_bytes() {
        // 64 个多字节字符 = 192 字节，但**字符数**是 64 —— 应通过。
        let key = "键".repeat(MAX_REQUEST_KEY_LEN);
        let parsed = RequestKey::new(&key).expect("应通过校验");

        assert_eq!(parsed.as_str().chars().count(), MAX_REQUEST_KEY_LEN);
    }

    #[test]
    fn submit_outcome_exposes_the_run_id_in_both_cases() {
        let run_id = RunId::new(7);

        assert_eq!(SubmitOutcome::Created(run_id).run_id(), run_id);
        assert_eq!(SubmitOutcome::Existing(run_id).run_id(), run_id);
    }

    #[test]
    fn submit_outcome_distinguishes_created_from_existing() {
        let run_id = RunId::new(7);

        assert!(SubmitOutcome::Created(run_id).is_created());
        assert!(!SubmitOutcome::Created(run_id).is_existing());
        assert!(SubmitOutcome::Existing(run_id).is_existing());
        assert!(!SubmitOutcome::Existing(run_id).is_created());
    }

    #[test]
    fn dedup_tuple_is_present_only_when_both_halves_are_known() {
        let mut msg = OutboxMessage {
            id: 1,
            run_id: Some(RunId::new(7)),
            seq: Some(3),
            topic: "run.event".into(),
            payload: Value::Null,
            dedup_key: None,
        };

        assert_eq!(msg.dedup_tuple(), Some((RunId::new(7), 3)));
        assert!(msg.has_dedup_tuple());

        // 只有 run_id 没有 seq —— 不构成完整去重元组。
        msg.seq = None;
        assert_eq!(msg.dedup_tuple(), None);
        assert!(!msg.has_dedup_tuple());

        // 只有 seq 没有 run_id —— 同样不完整。
        msg.run_id = None;
        msg.seq = Some(3);
        assert_eq!(msg.dedup_tuple(), None);
    }
}
