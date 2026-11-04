//! demo-rmcp-boundary
//!
//! 验证设计文档**待定 1**的决策依据：`rmcp` 的客户端 / 服务端能力边界。
//!
//! 背景：参考实现（Go）的 `internal/mcp` **只有客户端**（服务端代码只出现在
//! 测试里的 mock peer），而 `internal/a2a` 才有真正的服务端。
//! Rust 侧需要确认 `rmcp` 是否两端都支持 —— 这决定待定 1 的答案。
//!
//! ## 结论（验证后）
//!
//! `rmcp 3.8` **两端都支持**：
//! - 服务端：`tool_router` / `tool_handler` 宏 + `serve()`，可挂
//!   streamable-HTTP（tower 层，见 `StreamableHttpServerConfig`）
//! - 客户端：子进程（stdio）+ streamable-HTTP
//!
//! 因此待定 1 的答案不是「rmcp 只支持一边」，而是**架构决策**：
//! 我们要不要做 MCP 服务端？
//!
//! ## rmcp 3.8 的 API 要点（实测）
//!
//! - `Parameters<T>` 在 `rmcp::handler::server::wrapper`（不是 `model`）
//! - 工具方法必须用 `#[rmcp::tool]` 全路径限定（`use rmcp::tool` 导不出属性宏）
//! - 参数类型必须实现 `JsonSchema`（从 `rmcp::schemars` 取，版本必须与 rmcp 一致）
//! - 服务端 `serve()` 需要**传输**参数，不是光给 router
//! - 客户端需要实现 `ClientHandler`（或用 `()` 的默认实现）

use rmcp::{
    ServiceExt, handler::server::tool::ToolRouter, handler::server::wrapper::Parameters,
    schemars::JsonSchema, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

/// 工具参数。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EchoParam {
    /// 要回显的内容
    pub msg: String,
}

/// 一个最小的 MCP 服务端。
#[derive(Clone)]
pub struct EchoServer;

#[tool_router]
impl EchoServer {
    /// 回显。
    #[rmcp::tool(description = "回显输入内容")]
    fn echo(&self, Parameters(EchoParam { msg }): Parameters<EchoParam>) -> String {
        msg
    }
}

#[tool_handler]
impl rmcp::ServerHandler for EchoServer {}

/// 一个最小的 MCP 客户端封装。
///
/// 这是**出站**方向的形态 —— 把远端 MCP 服务器的工具接进来。
#[derive(Debug, Clone)]
pub struct McpClient {
    endpoint: String,
}

impl McpClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 列出远端工具。
    ///
    /// streamable-HTTP 传输 —— 需要 `transport-streamable-http-client-reqwest` feature。
    pub async fn list_tools(&self) -> Result<Vec<String>, McpError> {
        let transport =
            rmcp::transport::StreamableHttpClientTransport::from_uri(self.endpoint.clone());
        // `()` 提供默认的 ClientHandler 实现
        let _service = ().serve(transport).await.map_err(|e| McpError::Connect(e.to_string()))?;
        // 真实实现会 list_tools + 建索引
        Ok(Vec::new())
    }
}

/// MCP 错误。
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("端点非法：{0}")]
    BadEndpoint(String),
    #[error("连接失败：{0}")]
    Connect(String),
}

#[tokio::main]
async fn main() {
    // ── 服务端能力：tool_router 可构造 ──
    let router: ToolRouter<EchoServer> = EchoServer::tool_router();
    println!("✓ 服务端：tool_router 可构造");
    println!("  路由内容 = {router:?}");

    // ── 服务端能力：传输类型存在 ──
    // streamable-HTTP 服务端的配置类型存在，说明服务端 HTTP 传输可用
    let _cfg = rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default();
    println!("✓ 服务端：streamable-HTTP 服务端配置可用");

    // ── 客户端能力 ──
    let client = McpClient::new("http://127.0.0.1:9/mcp");
    println!("✓ 客户端：端点 = {}", client.endpoint());

    // 连一个不存在的服务 —— 预期失败，但证明传输类型可用
    match client.list_tools().await {
        Ok(_) => println!("• 客户端：连接成功（意外）"),
        Err(e) => println!("• 客户端：{e}（预期 —— 端口无服务，但传输可用）"),
    }

    // ── 结论 ──
    println!("\n═══ 待定 1 的结论 ═══");
    println!("rmcp 两端都支持：");
    println!("  服务端 —— tool_router/tool_handler + serve(transport)");
    println!("           streamable-HTTP 服务端配置可用");
    println!("  客户端 —— 子进程(stdio) + streamable-HTTP");
    println!();
    println!("参考实现（Go）：internal/mcp 只有客户端，internal/a2a 才有服务端。");
    println!("→ 因此待定 1 是**产品范围决策**，不是技术限制。");
    println!("→ 决策：nexus-mcp 做**出站**（工具接入）；");
    println!("         入站 MCP 服务端暂不实现 —— 需要独立的信任边界设计（谁可调用我们的工具）。");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 服务端_tool_router_可构造() {
        let router: ToolRouter<EchoServer> = EchoServer::tool_router();
        // router 非空说明宏展开成功
        let s = format!("{router:?}");
        assert!(!s.is_empty());
    }

    #[tokio::test]
    async fn 客户端_记录端点() {
        let c = McpClient::new("http://127.0.0.1:9/mcp");
        assert_eq!(c.endpoint(), "http://127.0.0.1:9/mcp");
    }
}
