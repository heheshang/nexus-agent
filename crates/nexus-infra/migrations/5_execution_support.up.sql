-- 5 · 执行支撑表
--
-- 对应参考实现：迁移 004 新建的
--   execution_checkpoints / execution_operations / execution_attempts /
--   execution_approvals / execution_events / execution_outbox
--
-- 全部挂在 `execution_runs` 上（引擎侧）——
-- 参考实现挂在 `workflow_runs` 上，因为那边只有一张表。

-- 检查点。版本绑定三元组 `(definition_hash, engine_version, graph_schema)`
-- （§4.1.1）—— 续跑时任一不同即拒绝。
CREATE TABLE execution_checkpoints (
    run_id          BIGINT      NOT NULL REFERENCES execution_runs (id),
    checkpoint_id   VARCHAR(512) NOT NULL,
    definition_hash CHAR(64)    NOT NULL,
    engine_version  VARCHAR(32) NOT NULL,
    graph_schema    INTEGER     NOT NULL,
    payload         BYTEA       NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 负载上限下沉到数据库（§3.9.2「约束下沉到数据库」）。
    CONSTRAINT execution_checkpoints_payload_check CHECK (octet_length(payload) <= 16777216)
);

-- 复合主键。PostgreSQL **不支持** `CREATE PRIMARY KEY ... ON ...` 这种独立语句 ——
-- 必须用 ALTER TABLE，或写在 CREATE TABLE 里。
ALTER TABLE execution_checkpoints
    ADD CONSTRAINT execution_checkpoints_pkey PRIMARY KEY (run_id, checkpoint_id);

-- 工具操作。`(run_id, path)` 唯一 —— 同一路径不会重复操作。
CREATE TABLE execution_operations (
    id            VARCHAR(64)   NOT NULL PRIMARY KEY,
    run_id        BIGINT        NOT NULL REFERENCES execution_runs (id),
    path          VARCHAR(1024) NOT NULL,
    kind          VARCHAR(32)   NOT NULL,
    args_hash     VARCHAR(64)   NOT NULL,
    -- 三种重放策略（§3.4.1）：readonly / idempotent / manual。
    replay_policy VARCHAR(24)   NOT NULL DEFAULT 'manual',
    status        VARCHAR(24)   NOT NULL,
    -- generation 快照 —— 僵尸写入的检测依据。
    generation    BIGINT        NOT NULL,
    output        JSONB,
    error         TEXT          NOT NULL DEFAULT '',
    attempt       INTEGER       NOT NULL DEFAULT 1,
    started_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ
);

CREATE UNIQUE INDEX execution_operations_run_id_path_key
    ON execution_operations (run_id, path);

-- 操作尝试。`(operation_id, attempt)` 唯一。
CREATE TABLE execution_attempts (
    id           BIGSERIAL PRIMARY KEY,
    operation_id VARCHAR(64) NOT NULL REFERENCES execution_operations (id),
    generation   BIGINT      NOT NULL,
    attempt      INTEGER     NOT NULL,
    status       VARCHAR(24) NOT NULL,
    error        TEXT        NOT NULL DEFAULT '',
    started_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ
);

CREATE UNIQUE INDEX execution_attempts_operation_id_attempt_key
    ON execution_attempts (operation_id, attempt);

-- 人工审批（HITL）。`decision` 的取值范围由 CHECK 约束下沉。
CREATE TABLE execution_approvals (
    id           BIGSERIAL PRIMARY KEY,
    run_id       BIGINT       NOT NULL REFERENCES execution_runs (id),
    interrupt_id VARCHAR(256) NOT NULL,
    path         VARCHAR(1024) NOT NULL,
    args_hash    VARCHAR(64)  NOT NULL,
    prompt       TEXT         NOT NULL,
    decision     VARCHAR(16)  NOT NULL DEFAULT 'pending',
    decided_by   BIGINT       REFERENCES users (id),
    decided_at   TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ  NOT NULL,
    comment      TEXT         NOT NULL DEFAULT '',
    CONSTRAINT execution_approvals_decision_check CHECK (
        decision IN ('pending', 'approved', 'rejected')
    )
);

-- 同一 run 内同一 interrupt 只审批一次。
CREATE UNIQUE INDEX execution_approvals_run_id_interrupt_id_key
    ON execution_approvals (run_id, interrupt_id);

-- 事件流。`(run_id, seq)` 是主键 —— seq 由数据库原子分配。
CREATE TABLE execution_events (
    run_id         BIGINT      NOT NULL REFERENCES execution_runs (id),
    seq            BIGINT      NOT NULL,
    type           VARCHAR(64) NOT NULL,
    schema_version INTEGER     NOT NULL DEFAULT 1,
    data           JSONB       NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 复合主键。用 ALTER TABLE —— PostgreSQL 不支持独立的
-- `CREATE PRIMARY KEY ... ON ...` 语句。
ALTER TABLE execution_events
    ADD CONSTRAINT execution_events_pkey PRIMARY KEY (run_id, seq);

-- 事务 outbox（§4.1.2）。事件与状态在**同一事务**写入这里，
-- 投递后标记 published_at。
--
-- 投递语义是**至少一次**（参考实现 outbox.go:28-29 注释原文）：
-- "Delivery is at least once: a crash after the peer accepts but before the PG
--  commit can redeliver the same (run_id,seq). Consumers must deduplicate by it."
-- 消费方**必须**按 (run_id, seq) 去重。
CREATE TABLE execution_outbox (
    id           BIGSERIAL PRIMARY KEY,
    run_id       BIGINT      REFERENCES execution_runs (id),
    seq          BIGINT,
    topic        VARCHAR(64) NOT NULL,
    payload      JSONB       NOT NULL,
    published_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    dedup_key    VARCHAR(160)
);

-- 消费方去重的依据 —— (run_id, seq) 唯一。
CREATE UNIQUE INDEX execution_outbox_run_id_seq_key ON execution_outbox (run_id, seq);

-- 投递去重键。
CREATE UNIQUE INDEX idx_outbox_dedup_key ON execution_outbox (dedup_key);

-- 只索引「待投递」的行 —— 部分索引。领取用 FOR UPDATE SKIP LOCKED。
CREATE INDEX idx_execution_outbox_pending ON execution_outbox (id)
    WHERE published_at IS NULL;
