-- 2 · Agent 与工具注册表
--
-- 对应参考实现：迁移 001 的 agents / tools，迁移 003（agent 归属），
-- 迁移 005（工具重放策略）的 replay_policy CHECK 约束，
-- 迁移 010（内置工具边界）的 mcp_server_id 外键，
-- 迁移 011（MCP 权威）给 tools 加的 mcp_* 字段。
--
-- 约束下沉到数据库：replay_policy 的取值范围由 CHECK 约束保证。

CREATE TABLE agents (
    id                      BIGSERIAL PRIMARY KEY,
    name                    VARCHAR(128) NOT NULL,
    description             TEXT,
    type                    VARCHAR(20)  NOT NULL DEFAULT 'expert',
    system_prompt           TEXT,
    model_config_id         BIGINT       NOT NULL REFERENCES model_configs (id),
    fallback_model_config_id BIGINT      REFERENCES model_configs (id),
    temperature             REAL         NOT NULL DEFAULT 0.7,
    top_p                   REAL         NOT NULL DEFAULT 0.9,
    max_tokens              INTEGER      NOT NULL DEFAULT 4096,
    max_iterations          INTEGER      NOT NULL DEFAULT 10,
    memory_enabled          BOOLEAN      NOT NULL DEFAULT true,
    memory_window           INTEGER      NOT NULL DEFAULT 20,
    memory_long_enabled     BOOLEAN      NOT NULL DEFAULT false,
    kb_ids                  JSONB        NOT NULL DEFAULT '[]'::jsonb,
    tools                   JSONB        NOT NULL DEFAULT '[]'::jsonb,
    is_preset               BOOLEAN      NOT NULL DEFAULT false,
    config                  JSONB        NOT NULL DEFAULT '{}'::jsonb,
    status                  SMALLINT     NOT NULL DEFAULT 1,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ  NOT NULL DEFAULT now(),
    owner_id                BIGINT       REFERENCES users (id) ON DELETE CASCADE
);

CREATE INDEX idx_agents_owner ON agents (owner_id);

-- MCP 服务器。参考实现迁移 011 给它加了 enabled / config_version /
-- encrypted_credentials / headers_present / allowed_user_ids / archived_at。
CREATE TABLE mcp_servers (
    id                   BIGSERIAL PRIMARY KEY,
    name                 VARCHAR(64)  NOT NULL,
    transport            VARCHAR(20)  NOT NULL,
    url                  VARCHAR(512) NOT NULL DEFAULT '',
    command              VARCHAR(512) NOT NULL DEFAULT '',
    args                 JSONB        NOT NULL DEFAULT '[]'::jsonb,
    env                  JSONB        NOT NULL DEFAULT '{}'::jsonb,
    headers              JSONB        NOT NULL DEFAULT '{}'::jsonb,
    status               VARCHAR(20)  NOT NULL DEFAULT 'disconnected',
    last_health_at       TIMESTAMPTZ,
    tools_cache          JSONB        NOT NULL DEFAULT '[]'::jsonb,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT now(),
    enabled              BOOLEAN      NOT NULL DEFAULT true,
    config_version       BIGINT       NOT NULL DEFAULT 1,
    -- 凭据由应用层用主密钥加密后写入（见 1_identity.up.sql 的注释）。
    encrypted_credentials TEXT       NOT NULL DEFAULT '',
    headers_present      BOOLEAN      NOT NULL DEFAULT false,
    allowed_user_ids     JSONB        NOT NULL DEFAULT '[]'::jsonb,
    tools_hash           VARCHAR(64)  NOT NULL DEFAULT '',
    archived_at          TIMESTAMPTZ,
    CONSTRAINT mcp_servers_config_version_check CHECK (config_version > 0)
);

-- 只索引「活跃」的服务器 —— 部分索引。
CREATE INDEX idx_mcp_active_servers ON mcp_servers (id)
    WHERE enabled AND archived_at IS NULL;

CREATE TABLE tools (
    id                 BIGSERIAL PRIMARY KEY,
    code               VARCHAR(64)  NOT NULL,
    name               VARCHAR(128) NOT NULL,
    type               VARCHAR(20)  NOT NULL,
    description        TEXT         NOT NULL,
    parameters         JSONB        NOT NULL,
    handler            VARCHAR(64),
    mcp_server_id      BIGINT       REFERENCES mcp_servers (id),
    endpoint           VARCHAR(512),
    auth_config        JSONB,
    default_config     JSONB        NOT NULL DEFAULT '{}'::jsonb,
    timeout_ms         INTEGER      NOT NULL DEFAULT 30000,
    is_dangerous       BOOLEAN      NOT NULL DEFAULT false,
    status             SMALLINT     NOT NULL DEFAULT 1,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ  NOT NULL DEFAULT now(),
    -- 迁移 005：三种重放策略。取值范围由 CHECK 约束下沉到数据库。
    replay_policy      VARCHAR(16)  NOT NULL DEFAULT 'manual',
    -- 迁移 011：MCP 工具的身份绑定。
    mcp_name           VARCHAR(128) NOT NULL DEFAULT '',
    mcp_config_version BIGINT       NOT NULL DEFAULT 0,
    binding_version    BIGINT       NOT NULL DEFAULT 0,
    policy_reviewed_by BIGINT       REFERENCES users (id),
    policy_reviewed_at TIMESTAMPTZ,
    replay_evidence    VARCHAR(512) NOT NULL DEFAULT '',
    CONSTRAINT tools_replay_policy_check CHECK (
        replay_policy IN ('readonly', 'idempotent', 'manual')
    )
);

CREATE UNIQUE INDEX tools_code_key ON tools (code);

-- 迁移 011：MCP 工具默认 is_dangerous=TRUE, replay_policy=manual。
-- 索引只覆盖「活跃的 MCP 工具」 —— 部分索引。
CREATE INDEX idx_mcp_active_tools ON tools (mcp_server_id, id)
    WHERE type = 'mcp' AND status = 1;

-- 同一 MCP 服务器内，工具名唯一 —— 部分唯一索引。
CREATE UNIQUE INDEX idx_mcp_tool_identity ON tools (mcp_server_id, mcp_name)
    WHERE type = 'mcp';
