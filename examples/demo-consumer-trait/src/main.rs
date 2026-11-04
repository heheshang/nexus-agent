//! demo-consumer-trait
//!
//! 验证设计文档 §3.1 / §4.1.2：**trait 由消费者定义**（consumer-defined trait），
//! 用它打破「能力层 ↔ 数据层」的潜在环。
//!
//! ## 要打破的环
//!
//! ```text
//! rag-engine 需要读写 chunk  →  需要 ChunkStore
//! repo 实现 ChunkStore       →  需要 rag-engine 的 Chunk 类型
//! ```
//!
//! 直接互依赖就是**环**。Go 靠隐式接口满足打破（`repo` 无需 import `rag`），
//! 但包循环依然存在 —— 改任一边都可能触发另一边的编译错误。
//!
//! ## Rust 的解法
//!
//! **端口在能力层，实现在数据层**：
//!
//! ```text
//! nexus-rag 定义 ChunkStore trait
//! nexus-repo 实现 ChunkStore（依赖 nexus-rag）
//! ```
//!
//! 依赖方向单向：`repo → rag`。Cargo 层面无环。

use std::sync::Arc;

// ═══════════════════════════════════════════════════════════
// 能力层（模拟 nexus-rag）：定义端口
// ═══════════════════════════════════════════════════════════

/// 文档分块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub id: i64,
    pub document_id: i64,
    pub text: String,
}

/// 存储错误。
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("存储失败：{0}")]
    Failed(String),
}

/// **端口定义在能力层**。
///
/// 数据层实现它。能力层**不**依赖数据层。
#[async_trait::async_trait]
pub trait ChunkStore: Send + Sync {
    /// 幂等写入分块。
    async fn upsert(&self, chunks: &[Chunk]) -> Result<(), StoreError>;

    /// 读取某文档的分块。
    async fn by_document(&self, document_id: i64) -> Result<Vec<Chunk>, StoreError>;
}

/// 检索引擎 —— 只依赖 `ChunkStore` 这个**trait**，不依赖任何具体实现。
pub struct RagEngine {
    store: Arc<dyn ChunkStore>,
}

impl RagEngine {
    pub fn new(store: Arc<dyn ChunkStore>) -> Self {
        Self { store }
    }

    pub async fn index(&self, chunks: Vec<Chunk>) -> Result<(), StoreError> {
        self.store.upsert(&chunks).await
    }

    pub async fn fetch(&self, document_id: i64) -> Result<Vec<Chunk>, StoreError> {
        self.store.by_document(document_id).await
    }
}

// ═══════════════════════════════════════════════════════════
// 数据层（模拟 nexus-repo）：实现端口
// ═══════════════════════════════════════════════════════════

/// PostgreSQL 实现。
pub struct PgChunkRepo {
    inner: std::sync::Mutex<Vec<Chunk>>,
}

impl PgChunkRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for PgChunkRepo {
    fn default() -> Self {
        Self {
            inner: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl ChunkStore for PgChunkRepo {
    async fn upsert(&self, chunks: &[Chunk]) -> Result<(), StoreError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Failed("锁中毒".into()))?;
        for c in chunks {
            // 幂等覆盖
            if let Some(pos) = g.iter().position(|x| x.id == c.id) {
                g[pos] = c.clone();
            } else {
                g.push(c.clone());
            }
        }
        Ok(())
    }

    async fn by_document(&self, document_id: i64) -> Result<Vec<Chunk>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Failed("锁中毒".into()))?;
        Ok(g.iter()
            .filter(|c| c.document_id == document_id)
            .cloned()
            .collect())
    }
}

/// 内存实现（测试用）。
pub struct MemChunkRepo {
    inner: std::sync::Mutex<Vec<Chunk>>,
}

impl MemChunkRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for MemChunkRepo {
    fn default() -> Self {
        Self {
            inner: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl ChunkStore for MemChunkRepo {
    async fn upsert(&self, chunks: &[Chunk]) -> Result<(), StoreError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Failed("锁中毒".into()))?;
        for c in chunks {
            if let Some(pos) = g.iter().position(|x| x.id == c.id) {
                g[pos] = c.clone();
            } else {
                g.push(c.clone());
            }
        }
        Ok(())
    }
    async fn by_document(&self, document_id: i64) -> Result<Vec<Chunk>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Failed("锁中毒".into()))?;
        Ok(g.iter()
            .filter(|c| c.document_id == document_id)
            .cloned()
            .collect())
    }
}

#[tokio::main]
async fn main() {
    // ── 依赖方向：数据层实现能力层的 trait ──
    let repo: Arc<dyn ChunkStore> = Arc::new(PgChunkRepo::new());
    let engine = RagEngine::new(Arc::clone(&repo));

    println!("✓ 依赖方向：repo 实现 rag 定义的 ChunkStore");
    println!("  引擎持有的是 Arc<dyn ChunkStore> —— 不依赖任何具体实现\n");

    // ── 功能验证 ──
    let chunks = vec![
        Chunk {
            id: 1,
            document_id: 100,
            text: "第一段".into(),
        },
        Chunk {
            id: 2,
            document_id: 100,
            text: "第二段".into(),
        },
        Chunk {
            id: 3,
            document_id: 200,
            text: "别的文档".into(),
        },
    ];
    engine.index(chunks.clone()).await.unwrap();

    let got = engine.fetch(100).await.unwrap();
    println!("✓ 索引并读取：document=100 → {} 个分块", got.len());
    for c in &got {
        println!("    id={} text={}", c.id, c.text);
    }
    assert_eq!(got.len(), 2);

    // ── 幂等覆盖 ──
    engine
        .index(vec![Chunk {
            id: 1,
            document_id: 100,
            text: "改过的第一段".into(),
        }])
        .await
        .unwrap();
    let got2 = engine.fetch(100).await.unwrap();
    assert_eq!(got2.len(), 2, "幂等覆盖不应增加条数");
    assert_eq!(got2[0].text, "改过的第一段");
    println!("✓ 幂等覆盖：条数不变 = {}", got2.len());

    // ── 换个实现，引擎不用改 ──
    let engine2 = RagEngine::new(Arc::new(MemChunkRepo::new()));
    engine2.index(chunks.clone()).await.unwrap();
    println!("✓ 替换实现无需改动引擎 —— 这就是端口的意义");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn data_layer_implements_capability_trait() {
        let repo: Arc<dyn ChunkStore> = Arc::new(PgChunkRepo::new());
        let engine = RagEngine::new(repo);
        engine
            .index(vec![Chunk {
                id: 1,
                document_id: 1,
                text: "x".into(),
            }])
            .await
            .unwrap();
        assert_eq!(engine.fetch(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn write_is_idempotent() {
        let repo = PgChunkRepo::new();
        repo.upsert(&[Chunk {
            id: 1,
            document_id: 1,
            text: "a".into(),
        }])
        .await
        .unwrap();
        repo.upsert(&[Chunk {
            id: 1,
            document_id: 1,
            text: "b".into(),
        }])
        .await
        .unwrap();
        let all = repo.by_document(1).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text, "b");
    }

    #[tokio::test]
    async fn implementation_swappable_without_touching_engine() {
        let e1 = RagEngine::new(Arc::new(PgChunkRepo::new()));
        let e2 = RagEngine::new(Arc::new(MemChunkRepo::new()));
        let c = vec![Chunk {
            id: 1,
            document_id: 1,
            text: "x".into(),
        }];
        e1.index(c.clone()).await.unwrap();
        e2.index(c).await.unwrap();
        assert_eq!(e1.fetch(1).await.unwrap().len(), 1);
        assert_eq!(e2.fetch(1).await.unwrap().len(), 1);
    }
}
