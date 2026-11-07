//! 图状态 —— **键 → JSON 值**的容器。
//!
//! # 本模块负责什么
//!
//! | 关注点 | 类型 |
//! |---|---|
//! | **状态容器** | [`GraphState`] |
//! | **状态大小上限**（4 MiB） | [`StateError::SizeExceeded`] |
//!
//! # 为什么用 `serde_json::Map` 而不是 `HashMap`
//!
//! Spec §3.4 明写：「图状态用 `serde_json::Map` —— 与 Go 版的 `map[string]any` 同构」。
//!
//! 本 workspace 的 `serde_json` 开了 `preserve_order` feature，
//! 所以 `serde_json::Map` 底层是 `IndexMap` —— **保序**。
//! 保序让状态的**序列化结果稳定** —— 那对「算哈希」与「对比快照」很重要。
//!
//! # ⚠️ 状态大小超限 → **错误**，不是**截断**（AC #4）
//!
//! 参考架构 §3.5.2 照抄 Go 版 `graph.go:367-369`：
//! 状态序列化后 > 4 MiB → **报错**。
//!
//! **截断**是比报错**更坏**的选择：
//! 被截断的状态看起来「还在」，但内容已经**悄悄少了一部分** ——
//! 下游节点读到缺失的键，行为不可预期，而且**没有报错**。
//!
//! 所以本模块的做法是：**先写、再量、超限则**回滚**（revert）。
//! 回滚之后状态与写入前**逐字节相同** —— 要么整次写入生效，要么完全没发生。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graph::MAX_STATE_BYTES;
use crate::node::NodeOutput;

/// **状态错误**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// **状态大小超限**。
    ///
    /// 携带实际字节数与上限 —— 报错要能指出「超了多少」。
    SizeExceeded {
        /// 实际字节数。
        size: usize,
        /// 上限（4 MiB）。
        limit: usize,
    },

    /// 状态**不是**合法 JSON（序列化失败）。
    NotSerializable {
        /// 原因。
        reason: String,
    },
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SizeExceeded { size, limit } => {
                write!(f, "状态大小 {size} 字节超过上限 {limit}")
            }
            Self::NotSerializable { reason } => write!(f, "状态无法序列化：{reason}"),
        }
    }
}

impl std::error::Error for StateError {}

impl From<StateError> for nexus_core::error::AppError {
    /// 把状态错误映射成**应用层**错误。
    ///
    /// # 映射规则
    ///
    /// | 状态错误 | 业务码 | 理由 |
    /// |---|---|---|
    /// | `SizeExceeded` | `BudgetExceeded`（30006） | 超出**预算** —— 与图编译的展开上限同一类 |
    /// | `NotSerializable` | `Internal`（90001） | 内部状态异常 |
    ///
    /// ⚠️ 与 [`crate::graph::CompileError`] 的映射**保持一致** ——
    /// 同一个物理上限（4 MiB）不该在两条路径上给出**不同的**码。
    fn from(err: StateError) -> Self {
        use nexus_core::error::{AppError, ErrorCode};
        let code = match &err {
            StateError::SizeExceeded { .. } => ErrorCode::BudgetExceeded,
            StateError::NotSerializable { .. } => ErrorCode::Internal,
        };
        AppError::new(code, err.to_string())
    }
}

/// **图状态** —— 键 → JSON 值。
///
/// # 为什么状态是**可变**的但**节点**看不见可变性
///
/// 节点通过 [`crate::node::NodeContext`] 拿到的是**只读**视图；
/// 节点产出 [`crate::node::NodeOutput`]，由驱动层合并进来。
///
/// 这让「并发节点同时改状态」变成**无锁**的：
/// 每个节点产出自己的结果，合并是**串行**的一步。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GraphState {
    /// 底层键值表 —— `serde_json::Map`（本 workspace 开了 `preserve_order`，**保序**）。
    inner: serde_json::Map<String, Value>,
}

impl GraphState {
    /// 构造（空状态）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 从一份键值表构造。
    pub fn from_map(inner: serde_json::Map<String, Value>) -> Self {
        Self { inner }
    }

    /// 取一个键。
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.inner.get(key)
    }

    /// 是否含某个键。
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    /// 键的数量。
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 全部键。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
    }

    /// 全部键值对。
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.inner.iter()
    }

    /// 底层键值表。
    pub fn as_map(&self) -> &serde_json::Map<String, Value> {
        &self.inner
    }

    /// 底层键值表（可变）—— ⚠️ **绕过**大小检查。
    ///
    /// 只给「已经检查过大小」的内部路径用（如反序列化已有快照）。
    /// 运行时写入**必须**走 [`GraphState::insert`] / [`GraphState::apply`]。
    pub fn as_map_mut(&mut self) -> &mut serde_json::Map<String, Value> {
        &mut self.inner
    }

    /// 写入一个键。
    ///
    /// # 语义（AC #4）
    ///
    /// 写入后状态大小 > 4 MiB → **报错**且**回滚** ——
    /// 状态与调用前**逐字节相同**。
    ///
    /// # 错误
    ///
    /// - 写入后大小 > [`MAX_STATE_BYTES`] → [`StateError::SizeExceeded`]
    /// - 状态无法序列化 → [`StateError::NotSerializable`]
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Result<(), StateError> {
        let key = key.into();
        let previous = self.inner.insert(key.clone(), value);

        match self.check_size() {
            Ok(()) => Ok(()),
            Err(err) => {
                // ── 回滚 ──
                // 恢复到写入前的形态：原来有值就放回，原来没有就删掉。
                match previous {
                    Some(old) => {
                        self.inner.insert(key, old);
                    }
                    None => {
                        self.inner.remove(&key);
                    }
                }
                Err(err)
            }
        }
    }

    /// 合并一份**节点产出**。
    ///
    /// # 语义
    ///
    /// 产出里的每个键都写进状态；**整体**要么生效、要么**完全回滚**。
    ///
    /// # ⚠️ 为什么是**全有或全无**
    ///
    /// 半应用的状态 = 节点 A 的产出进了、节点 B 的没进 ——
    /// 而节点之间是有**数据依赖**的（B 可能读 A 的产出）。
    /// 半应用会让图进入一个**从未定义过**的状态。
    pub fn apply(&mut self, output: &NodeOutput) -> Result<(), StateError> {
        if output.is_empty() {
            return Ok(());
        }

        // ── 记录被覆盖的旧值 ──
        let mut previous: Vec<(String, Option<Value>)> = Vec::new();
        for key in output.values().keys() {
            previous.push((key.clone(), self.inner.get(key).cloned()));
        }

        for (key, value) in output.values() {
            self.inner.insert(key.clone(), value.clone());
        }

        match self.check_size() {
            Ok(()) => Ok(()),
            Err(err) => {
                // ── 整体回滚 ──
                for (key, old) in previous {
                    match old {
                        Some(value) => {
                            self.inner.insert(key, value);
                        }
                        None => {
                            self.inner.remove(&key);
                        }
                    }
                }
                Err(err)
            }
        }
    }

    /// 当前的**序列化字节数**。
    ///
    /// # 与参考实现的一致性
    ///
    /// 参考架构 §3.5.2 照抄 Go 版 `graph.go:367-369`：
    /// 用 `serde_json::to_vec(&state).len()`。
    ///
    /// 本实现序列化的是**同一个**东西（整个状态对象）——
    /// 不是「每个值分别量再求和」。后者会漏掉键名与分隔符的字节。
    pub fn size_bytes(&self) -> Result<usize, StateError> {
        let bytes = serde_json::to_vec(&self.inner).map_err(|e| StateError::NotSerializable {
            reason: e.to_string(),
        })?;
        Ok(bytes.len())
    }

    /// 检查状态大小是否超限。
    ///
    /// # 错误
    ///
    /// 超过 [`MAX_STATE_BYTES`] → [`StateError::SizeExceeded`]。
    pub fn check_size(&self) -> Result<(), StateError> {
        let size = self.size_bytes()?;
        if size > MAX_STATE_BYTES {
            return Err(StateError::SizeExceeded {
                size,
                limit: MAX_STATE_BYTES,
            });
        }
        Ok(())
    }
}
