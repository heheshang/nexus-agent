//! 统一响应包 —— 对外响应形状的**唯一**来源。
//!
//! # 形状
//!
//! ```json
//! {"code": 0, "message": "ok", "data": {...}}
//! ```
//!
//! | 情况 | `code` | `message` | `data` |
//! |---|---|---|---|
//! | 成功 | **0** | `"ok"` | 有则带上，无则**省略** |
//! | 失败 | **业务码**（如 `90001`） | 面向用户的文案 | **省略** |
//!
//! # 为什么成功用 `0`（决策 D11）
//!
//! 1. 参考实现 `internal/pkg/response` **已经这么用** ——
//!    前端**已经**按 `code == 0` 判成功。改掉是**协议破坏性变更**。
//! 2. 业务码从 `10001` 起 —— `0` **不与任何业务码冲突**。
//! 3. 码段（`1xxxx` 参数 / `2xxxx` 鉴权 / …）描述的是**错误**的分类，
//!    成功**不属于**任何错误码段 —— `0` 是「不属于任何错误码段」的**自然**表达。
//!
//! # 为什么 `code` 是**数字**
//!
//! 架构文档 §2.6.3 的**验证记录**（2026-11-04）：
//!
//! > 派生 `Serialize` 会输出 `{"code":"InvalidParam"}`；手工实现后输出 `{"code":10001}`。
//! > 这个差异看似细节，实际决定了前端能否按码段分支。
//!
//! `nexus_core::ErrorCode` 已经**手工**实现了 `Serialize`（输出数字）——
//! 所以本模块直接复用它，不自己造一个 `code` 类型。
//!
//! # 内部原因绝不外泄
//!
//! 架构文档 §2.6.5 —— 响应体**只**序列化 `UserFacing`
//! （`code` + `message`），**没有** `cause` 字段。
//!
//! 这不是「记得别加」—— 是**类型上**就不可能：
//! `UserFacing` 只有两个字段，而 `AppError` 根本不实现 `Serialize`。

use nexus_core::error::{ErrorCode, UserFacing};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 成功的业务码。
///
/// **0** —— 不与任何业务码冲突（业务码从 `10001` 起）。
pub const SUCCESS_CODE: i32 = 0;

/// 成功响应的默认文案。
pub const SUCCESS_MESSAGE: &str = "ok";

/// 统一响应包。
///
/// # `data` 的序列化
///
/// `Option<Value>` 且带 `skip_serializing_if` ——
/// `None` 时**整个字段被省略**，不输出 `"data": null`。
///
/// 理由：前端的类型定义通常是 `data?: T`（可选），
/// `"data": null` 会让 `data` 的类型变成 `T | null`，**增加前端的处理分支**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiResponse {
    /// 业务码。成功是 [`SUCCESS_CODE`]（0），失败是业务码。
    pub code: i32,
    /// 面向用户的消息。
    pub message: String,
    /// 业务数据。缺省时**整个字段被省略**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ApiResponse {
    /// 构造一个**成功**响应（无数据）。
    pub fn ok() -> Self {
        Self {
            code: SUCCESS_CODE,
            message: SUCCESS_MESSAGE.to_string(),
            data: None,
        }
    }

    /// 构造一个**成功**响应（带数据）。
    pub fn ok_with(data: impl Serialize) -> Self {
        Self {
            code: SUCCESS_CODE,
            message: SUCCESS_MESSAGE.to_string(),
            data: serde_json::to_value(data).ok(),
        }
    }

    /// 构造一个**失败**响应。
    ///
    /// ⚠️ `message` 必须是**面向用户**的文案 —— 内部原因**绝不**进来。
    pub fn failure(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.code(),
            message: message.into(),
            data: None,
        }
    }

    /// 构造一个**失败**响应（文案已是 `Cow<'static, str>`）。
    pub fn failure_cow(code: ErrorCode, message: std::borrow::Cow<'static, str>) -> Self {
        Self {
            code: code.code(),
            message: message.into_owned(),
            data: None,
        }
    }

    /// 从 `UserFacing` 构造**失败**响应。
    ///
    /// 这是**唯一**应该从 `AppError` 取信息的入口 ——
    /// 它只接受 `UserFacing`，所以 `cause` **没有**传进来的路径。
    pub fn from_facing(facing: &UserFacing) -> Self {
        Self::failure_cow(facing.code, facing.message.clone())
    }

    /// 序列化响应体。
    ///
    /// # 兜底
    ///
    /// 理论上不可达（字段都是平凡类型），
    /// 但兜底**必须有** —— 一个**空 body** 会让客户端拿到 0 字节，
    /// 那比一个格式错误的 body 更难排查。
    ///
    /// ⚠️ 不能用 `br#"..."#` —— 字节串字面量**必须**是 ASCII，
    /// 而兜底消息是中文。
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self)
            .unwrap_or_else(|_| r#"{"code":90001,"message":"系统内部错误"}"#.as_bytes().to_vec())
    }
}

/// 成功响应的构造器 —— 返回**统一响应包**。
pub fn ok_response() -> ApiResponse {
    ApiResponse::ok()
}

/// 成功响应（带数据）。
pub fn ok_with<T: Serialize>(data: T) -> ApiResponse {
    ApiResponse::ok_with(data)
}

/// 失败响应。
pub fn failure_response(code: ErrorCode, message: impl Into<String>) -> ApiResponse {
    ApiResponse::failure(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_success_response_carries_the_numeric_zero_code() {
        let resp = ApiResponse::ok();
        assert_eq!(resp.code, 0, "成功的 code 必须是数字 0");
        assert_eq!(resp.message, "ok");
        assert!(resp.data.is_none());
    }

    #[test]
    fn a_failure_response_carries_the_business_code() {
        let resp = ApiResponse::failure(ErrorCode::Unauthorized, "未认证");
        assert_eq!(resp.code, 20101);
        assert_eq!(resp.message, "未认证");
    }

    #[test]
    fn an_absent_data_field_is_omitted() {
        let bytes = String::from_utf8(ApiResponse::ok().to_bytes()).unwrap();
        assert_eq!(bytes, r#"{"code":0,"message":"ok"}"#);
        assert!(
            !bytes.contains("data"),
            "data 缺省时应整个省略，不输出 null：{bytes}"
        );
    }

    #[test]
    fn the_data_field_is_present_when_given() {
        let resp = ApiResponse::ok_with(json!({"id": 7}));
        let bytes = String::from_utf8(resp.to_bytes()).unwrap();
        assert!(bytes.contains(r#""data":{"id":7}"#), "实际：{bytes}");
    }

    #[test]
    fn the_code_is_serialized_as_a_number() {
        let bytes =
            String::from_utf8(ApiResponse::failure(ErrorCode::Internal, "x").to_bytes()).unwrap();
        assert!(bytes.contains(r#""code":90001"#), "实际：{bytes}");
        assert!(
            !bytes.contains(r#""code":"Internal""#),
            "code 必须是数字，不能是变体名：{bytes}"
        );
    }

    #[test]
    fn the_response_body_carries_no_internal_cause() {
        // `from_facing` 是唯一入口，且只接受 `UserFacing` ——
        // 类型上没有传 `cause` 的路径。
        let facing = UserFacing::new(ErrorCode::Internal, "系统内部错误");
        let bytes = String::from_utf8(ApiResponse::from_facing(&facing).to_bytes()).unwrap();
        assert!(!bytes.contains("cause"), "实际：{bytes}");
        assert!(!bytes.contains("source"), "实际：{bytes}");
    }

    #[test]
    fn the_fallback_body_is_still_valid_json() {
        // 兜底路径不能被构造出来（字段都是平凡类型），
        // 但**形状**必须是对的 —— 否则兜底本身就会产出坏 body。
        let fallback = r#"{"code":90001,"message":"系统内部错误"}"#;
        let v: Value = serde_json::from_str(fallback).expect("兜底 body 必须是合法 JSON");
        assert_eq!(v["code"], json!(90001));
    }
}
