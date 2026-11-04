-- 3 · 工作流定义（产品侧）
--
-- 对应参考实现：迁移 006（工作流产品化）的 workflows / workflow_versions /
-- workflow_step_runs 三张表。
--
-- 本迁移只放**产品侧**的定义与轨迹。
-- 引擎侧的执行状态在 4_execution.up.sql —— 见该文件顶部的「两表分离」说明。

CREATE TABLE workflows (
    id          BIGSERIAL PRIMARY KEY,
    name        VARCHAR(128) NOT NULL,
    description TEXT,
    -- DSL 文本（用户写的定义）。
    dsl         TEXT         NOT NULL,
    version     INTEGER      NOT NULL DEFAULT 1,
    is_active   BOOLEAN      NOT NULL DEFAULT false,
    creator_id  BIGINT       NOT NULL REFERENCES users (id),
    status      SMALLINT     NOT NULL DEFAULT 1,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ
);

-- 冻结定义快照。`(workflow_id, definition_hash)` 唯一 ——
-- 同一个工作流的同一个定义哈希只存一份。
--
-- 这支撑设计文档 §2.5.2「冻结定义 + 哈希绑定」：
-- 跑起来之后工作流定义可能被改，靠哈希检测到「定义被改过」。
CREATE TABLE workflow_versions (
    id              BIGSERIAL PRIMARY KEY,
    workflow_id     BIGINT      NOT NULL REFERENCES workflows (id),
    version         INTEGER     NOT NULL,
    definition_hash CHAR(64)    NOT NULL,
    definition      JSONB       NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX workflow_versions_workflow_id_definition_hash_key
    ON workflow_versions (workflow_id, definition_hash);
