//! 负向样例：事件负载的字段名写错。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.2.4 的核心主张 —— **构造事件时字段名写错 → 编译错误**。
//!
//! 参考实现用 `map[string]any` 作负载，字段名写错**不会报错**，只会在前端运行时
//! 变成 `undefined`。本设计用类型化 enum 消灭了这类 bug。
//!
//! 期望的错误：
//! ```text
//! error[E0560]: struct `ToolResult` has no field named `ms`
//! ```

use nexus_core::{EventPayload, ToolStatus};
use nexus_core::ToolCallId;

fn main() {
    // ❌ 字段名写错：应为 `elapsed_ms`，不是 `ms`。
    let _payload = EventPayload::ToolResult {
        call_id: ToolCallId::new("c1"),
        result: "ok".into(),
        ms: 12,
        status: ToolStatus::Ok,
    };
}
