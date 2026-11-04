-- 4 · 执行引擎侧（引擎侧）
--
-- ══════════════════════════════════════════════════════════════════════
-- 「两表分离」—— 对参考实现的刻意偏离
-- ══════════════════════════════════════════════════════════════════════
--
-- 参考实现（迁移 004，持久执行）给 `workflow_runs` **加 11 列**：
--   definition / definition_hash / engine_version / graph_schema /
--   state_version / generation / lease_owner / lease_until /
--   deadline_at / available_at / next_seq
--
-- 那是**历史兼容**的做法 —— 一张表同时当「旧工作流模型」和「持久执行模型」，
-- 于是 `workflow_runs` 既有产品语义（trigger_type / input / output / error），
-- 又有引擎语义（generation / lease_owner / next_seq）。
--
-- Rust 版**拆成两张表**（设计文档 §3.9.4）：
--   · `execution_runs` —— 引擎侧：租约、fencing、状态机、序号分配
--   · `workflow_runs` —— 产品侧：一次运行的业务视图
--
-- 理由：**避免历史数据迁移的复杂度**。产品侧与引擎侧的生命周期不同 ——
-- 产品侧关心「这次运行的结果是什么」，引擎侧关心「谁在跑、跑了多久、
-- 写到哪一步了」。混在一张表里，两边的字段增速与变更节奏会互相拖累。
--
-- ⚠️ 两张表**不是**同一份数据的两份拷贝：
--   · `execution_runs` 是**权威**（PG 是唯一事实来源）
--   · `workflow_runs` 是**产品视图**，由执行层在状态变更时同步写入
-- 写入权唯一（§4.2 差异 1）：`nexus-execution` 是 `workflow_runs` 的唯一写入者。
--
-- ══════════════════════════════════════════════════════════════════════
--
-- 对应参考实现：迁移 004 的 execution_* 五张表 + 迁移 006 的 workflow_runs
-- 产品侧字段。

-- ── 引擎侧：执行的权威状态 ──────────────────────────────────────────
CREATE TABLE execution_runs (
    id              BIGSERIAL PRIMARY KEY,
    -- 冻结的定义快照（§2.5.2 三件套）。
    definition      JSONB        NOT NULL,
    definition_hash CHAR(64)     NOT NULL,
    engine_version  VARCHAR(32)  NOT NULL,
    graph_schema    INTEGER      NOT NULL DEFAULT 1,
    -- 状态机。
    status          VARCHAR(24)  NOT NULL DEFAULT 'queued',
    -- 租约与 fencing：generation 递增即作废旧持有者（§2.5.4）。
    generation      BIGINT       NOT NULL DEFAULT 0,
    lease_owner     VARCHAR(128) NOT NULL DEFAULT '',
    lease_until     TIMESTAMPTZ,
    deadline_at     TIMESTAMPTZ  NOT NULL DEFAULT (now() + INTERVAL '1 day'),
    available_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    -- 序号分配。`next_seq = next_seq + 1 RETURNING next_seq` 是原子的
    -- （参考实现 `internal/execution/store.go:421`）—— 跨进程也要单调，
    -- 所以序号必须由**数据库**分配，不能靠应用层自增。
    next_seq        BIGINT       NOT NULL DEFAULT 0,
    state_version   BIGINT       NOT NULL DEFAULT 1,
    pending_ids     JSONB        NOT NULL DEFAULT '[]'::jsonb,
    -- 幂等键。§4.1.1：`(owner_id, request_key)` 唯一，
    -- 重复提交返回既有 run（`ON CONFLICT ... DO NOTHING`）。
    owner_id        BIGINT       NOT NULL REFERENCES users (id),
    request_key     VARCHAR(64)  NOT NULL,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- 幂等键唯一约束（§4.1.1）。
CREATE UNIQUE INDEX idx_execution_request_key ON execution_runs (owner_id, request_key);

-- 只索引「就绪」的行 —— 部分索引（§3.9.1 关键索引模式）。
-- worker 领取时只关心 queued / running / retry_wait。
CREATE INDEX idx_execution_ready ON execution_runs (available_at, id)
    WHERE status IN ('queued', 'running', 'retry_wait');

-- 活跃 run 的租约视图 —— 部分索引。
CREATE INDEX idx_execution_active_owner ON execution_runs (owner_id, lease_until)
    WHERE status = 'running';

-- 未结束的 run（用于超时巡检）—— 部分索引。
CREATE INDEX idx_execution_live_deadline ON execution_runs (deadline_at, id)
    WHERE status IN ('queued', 'running', 'retry_wait', 'waiting_approval', 'needs_attention');

-- ── 产品侧：运行的业务视图 ──────────────────────────────────────────
CREATE TABLE workflow_runs (
    id            BIGSERIAL PRIMARY KEY,
    -- 引擎侧 run —— 产品视图挂在它上面。
    execution_id  BIGINT       NOT NULL REFERENCES execution_runs (id),
    workflow_id   BIGINT       NOT NULL REFERENCES workflows (id),
    trigger_type  VARCHAR(20)  NOT NULL DEFAULT 'manual',
    status        VARCHAR(24)  NOT NULL DEFAULT 'running',
    input         JSONB        NOT NULL DEFAULT '{}'::jsonb,
    output        JSONB,
    error         VARCHAR(512) NOT NULL DEFAULT '',
    trace_id      VARCHAR(64)  NOT NULL DEFAULT '',
    owner_id      BIGINT       REFERENCES users (id),
    -- 冻结定义指向的版本快照。
    definition_release_id BIGINT REFERENCES workflow_versions (id),
    started_at    TIMESTAMPTZ,
    finished_at   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX idx_workflow_runs_status ON workflow_runs (status);
CREATE INDEX idx_workflow_runs_execution ON workflow_runs (execution_id);

-- ── 步骤轨迹 ────────────────────────────────────────────────────────
CREATE TABLE workflow_step_runs (
    id          BIGSERIAL PRIMARY KEY,
    run_id      BIGINT       NOT NULL REFERENCES workflow_runs (id),
    node_key    VARCHAR(64)  NOT NULL,
    node_type   VARCHAR(24)  NOT NULL,
    status      VARCHAR(20)  NOT NULL DEFAULT 'running',
    input       JSONB,
    output      JSONB,
    error       VARCHAR(512) NOT NULL DEFAULT '',
    tokens      INTEGER      NOT NULL DEFAULT 0,
    started_at  TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);

CREATE INDEX idx_step_runs_run ON workflow_step_runs (run_id);
