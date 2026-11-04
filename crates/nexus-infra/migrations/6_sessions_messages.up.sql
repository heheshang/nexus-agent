-- 6 · 会话与消息
--
-- 对应参考实现：迁移 001 的 sessions / messages 两张表，
-- 以及迁移 009（持久记忆）给 messages 相关的部分索引。
--
-- 会话 ID 是 UUID 而非自增 —— `nexus-core::id::SessionId` 已是 UUID 类型。
-- 理由（§2.8.1）：会话 ID 会返回给客户端，客户端可能**提前**用它发起请求。

CREATE TABLE sessions (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      BIGINT      NOT NULL REFERENCES users (id),
    agent_id     BIGINT      REFERENCES agents (id),
    title        VARCHAR(256) NOT NULL DEFAULT '新对话',
    status       VARCHAR(20)  NOT NULL DEFAULT 'active',
    pinned       BOOLEAN      NOT NULL DEFAULT false,
    message_count INTEGER    NOT NULL DEFAULT 0,
    token_usage  BIGINT       NOT NULL DEFAULT 0,
    last_msg_at  TIMESTAMPTZ,
    metadata     JSONB        NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- 会话列表按最近活跃排序 —— 部分索引（只索引有消息的会话）。
CREATE INDEX idx_sessions_user ON sessions (user_id, last_msg_at DESC);

CREATE TABLE messages (
    id               BIGSERIAL PRIMARY KEY,
    session_id       UUID        NOT NULL REFERENCES sessions (id),
    role             VARCHAR(20) NOT NULL,
    content          TEXT        NOT NULL,
    reasoning_content TEXT,
    tool_calls       JSONB,
    tool_call_id     VARCHAR(128),
    refs             JSONB,
    model_name       VARCHAR(64),
    prompt_tokens    INTEGER     NOT NULL DEFAULT 0,
    completion_tokens INTEGER    NOT NULL DEFAULT 0,
    first_token_ms   INTEGER     NOT NULL DEFAULT 0,
    latency_ms       INTEGER     NOT NULL DEFAULT 0,
    status           SMALLINT    NOT NULL DEFAULT 1,
    trace_id         VARCHAR(64) NOT NULL DEFAULT '',
    metadata         JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_session ON messages (session_id, created_at);

-- 记忆游标扫描 —— 部分索引：只索引 user/assistant 角色且有效的消息。
CREATE INDEX idx_messages_memory_cursor ON messages (session_id, id)
    WHERE role IN ('user', 'assistant') AND status = 1;
