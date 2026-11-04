-- 1 · 身份与模型配置
--
-- 对应参考实现：迁移 001（初始 schema）中的 users / model_providers / model_configs，
-- 以及迁移 003（agent 归属）给 agents 加 owner_id 所依赖的 users 表。
--
-- 本迁移建立「谁」与「用什么模型」两条基础线。

CREATE TABLE users (
    id            BIGSERIAL PRIMARY KEY,
    username      VARCHAR(64)  NOT NULL,
    email         VARCHAR(128) NOT NULL,
    password_hash VARCHAR(256) NOT NULL,
    role          VARCHAR(20)  NOT NULL DEFAULT 'user',
    avatar_url    VARCHAR(512) NOT NULL DEFAULT '',
    status        SMALLINT     NOT NULL DEFAULT 1,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX users_username_key ON users (username);
CREATE UNIQUE INDEX users_email_key    ON users (email);

CREATE TABLE model_providers (
    id         BIGSERIAL PRIMARY KEY,
    code       VARCHAR(32)  NOT NULL,
    name       VARCHAR(64)  NOT NULL,
    base_url   VARCHAR(512) NOT NULL,
    -- 凭据**不在 SQL 里加密**（参考实现迁移 011 注释原文）：
    -- "Credentials cannot be encrypted by SQL without the application's master key.
    --  Upgrade them through the explicit credential migration/admin sync action"
    api_key    TEXT         NOT NULL DEFAULT '',
    status     SMALLINT     NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX model_providers_code_key ON model_providers (code);

CREATE TABLE model_configs (
    id                  BIGSERIAL PRIMARY KEY,
    provider_id         BIGINT         NOT NULL REFERENCES model_providers (id),
    model_name          VARCHAR(64)    NOT NULL,
    alias               VARCHAR(64)    NOT NULL,
    type                VARCHAR(16)    NOT NULL,
    context_window      INTEGER        NOT NULL DEFAULT 8192,
    max_output          INTEGER        NOT NULL DEFAULT 4096,
    input_price         NUMERIC(12, 6) NOT NULL DEFAULT 0,
    output_price        NUMERIC(12, 6) NOT NULL DEFAULT 0,
    priority            INTEGER        NOT NULL DEFAULT 100,
    capabilities        JSONB          NOT NULL DEFAULT '[]'::jsonb,
    is_default          BOOLEAN        NOT NULL DEFAULT false,
    status              SMALLINT       NOT NULL DEFAULT 1,
    created_at          TIMESTAMPTZ    NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ    NOT NULL DEFAULT now(),
    embedding_dimension INTEGER        NOT NULL DEFAULT 0
);

CREATE INDEX idx_model_configs_type ON model_configs (type, priority);
