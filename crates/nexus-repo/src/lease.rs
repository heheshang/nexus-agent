//! 租约与 **generation fencing** 的**类型**部分。
//!
//! # 机制（架构文档 §2.5.4、参考架构文档 §3.4.2）
//!
//! 执行模型的**核心不变量**：worker 用 `FOR UPDATE SKIP LOCKED` 领取任务，
//! 写 `lease_owner` + `lease_until`，每次写入都带 `generation` 自增。
//!
//! ```text
//! 领取：  UPDATE ... SET lease_owner=$w, generation = generation + 1, lease_until = now() + 30s
//! 写入：  UPDATE ... WHERE id = $run AND generation = $my_generation
//! ```
//!
//! worker 崩溃后被别人重新领取 → `generation` 递增 →
//! 旧 worker 的后续写入**影响 0 行** —— 它会发现自己是僵尸。
//!
//! # 关键设计：`generation` 封装在租约对象里
//!
//! §2.5.4 明确：**`generation` 不应该由调用方手动传** ——
//! 应该由**租约对象**持有。
//!
//! [`RunLease`] 持有 `generation`，并且**不提供**返回裸 generation 的公开方法。
//! 写入方法要求 `&RunLease` —— 调用方**写不出**「忘记带 generation」的 UPDATE。
//!
//! 这与 P1 的「`EventSink` 不可 Clone，序号与写入顺序一致」是同一种思路 ——
//! **把不变量塞进类型，而不是靠约定**。
//!
//! # 租约过期判定必须用 `clock_timestamp()`
//!
//! PostgreSQL 的 `now()` 返回**事务开始时间**，在事务内是**固定值**。
//! 用它判断租约是否过期会得到错误结果 ——
//! 事务跑了 5 分钟，`now()` 仍停在 5 分钟前，租约看起来「永不过期」。
//!
//! `clock_timestamp()` 返回**实际当前时间**。
//! 参考实现的 fencing 条件用的就是 `clock_timestamp()`（`store.go:198`）。
//!
//! # 本 crate **不碰** `workflow_runs`
//!
//! 架构文档 §4.2 差异 1：`workflow_runs` 由 `nexus-execution` **独占**。
//! 本模块操作的是 `execution_runs` —— NEX-9 建的**引擎侧**独立表。

use nexus_core::error::{AppError, ErrorCode};
use nexus_core::id::RunId;

/// 围栏条件。
///
/// 承载 fencing 的三个要素：`id` / `generation` / `lease_owner`。
///
/// # 为什么存在这个类型
///
/// `sqlx` 没有 GORM 的链式 `Where` 复用机制 —— 参考架构文档 §3.4.2 建议
/// 把 fencing 条件封装成常量片段或结构体。
///
/// 本实现把条件**定义在唯一一个地方** —— [`Fence::push_conditions`]。
/// 所有围栏写入都经由它拼装 SQL，所以「加了条件忘了同步别处」不可能发生。
///
/// # 字段为什么是 `pub`
///
/// AC #1 要求 `Fence` 承载这三个字段。公开它们让围栏条件可以被
/// 检查、被测试、被构造。
///
/// ⚠️ 但**正常路径**上的写入方法要求 `&RunLease`（见模块文档）——
/// `Fence` 主要是**内部复用**与**测试构造**用的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// 目标 run 的 ID。
    pub id: RunId,
    /// 世代号 —— 领取时自增，fencing 的依据。
    pub generation: i64,
    /// 租约持有者标识。
    pub lease_owner: String,
}

impl Fence {
    /// 构造围栏。
    pub fn new(id: RunId, generation: i64, lease_owner: impl Into<String>) -> Self {
        Self {
            id,
            generation,
            lease_owner: lease_owner.into(),
        }
    }

    /// 围栏条件的 **SQL 模板**。
    ///
    /// `?` 是**绑定点**标记 —— 不是一个真的会出现在 SQL 里的字符。
    /// [`Fence::push_conditions`] 按 `?` 的出现顺序把三个值绑定进去，
    /// 由 sqlx 自动编号成 `$1` / `$2` / `$3`。
    ///
    /// # 为什么用 `?` 而不是 `$1`
    ///
    /// `$1` / `$2` / `$3` 是**绝对**编号 —— 只有当围栏的三个参数恰好是
    /// 查询里的**前三个**参数时才对。而围栏写入的语句形态不止一种
    /// （`SET status = ?`、`SET next_seq = next_seq + ?`），
    /// 参数个数与顺序各不相同 —— 手写绝对编号必然漂移，
    /// 漂移的后果是「语法错误」或 worse，「参数错位」。
    ///
    /// `?` 是**相对**的 —— 绑定顺序即顺序，编号由 sqlx 负责。
    ///
    /// # 为什么是「一处定义」
    ///
    /// 模板 + 逐段 `push` 并存会产生**两份**定义 ——
    /// 改了一处忘了另一处，就是「fencing 静默失效」的入口。
    ///
    /// 本实现只保留**一份**：模板是**唯一**的真相，
    /// [`Fence::push_conditions`] 按 `?` 切分它并绑定值。
    /// 测试断言的也是**同一个模板** —— 两者不可能漂移。
    ///
    /// # 六个条件各自挡住什么
    ///
    /// | 条件 | 挡住什么 |
    /// |---|---|
    /// | `id = ?` | 写错行 |
    /// | `generation = ?` | **僵尸写入**（核心） |
    /// | `lease_owner = ?` | 别人用**同一个 generation** 写（不该发生，但多一道锁） |
    /// | `status = 'running'` | 写已终态的 run |
    /// | `lease_until > clock_timestamp()` | 租约**已过期** —— 即使 generation 还对 |
    /// | `deadline_at > clock_timestamp()` | run 已过总截止时间 |
    ///
    /// 后两条是 `generation` 之外的**第二道防线**：
    /// 一个持有者即使 generation 正确，租约到期后也不该继续写。
    ///
    /// # 为什么用 `clock_timestamp()` 而不是 `now()`
    ///
    /// `now()` 是**事务开始时间**，在事务内是固定值。
    /// 长事务里用它判断租约是否过期会得到错误结果。
    /// `clock_timestamp()` 是**实际当前时间**。
    ///
    /// 参考实现的 fencing 条件用的就是 `clock_timestamp()`（`store.go:198`）。
    pub const CONDITIONS_TEMPLATE: &'static str = "id = ? AND generation = ? AND lease_owner = ? \
         AND status = 'running' \
         AND lease_until > clock_timestamp() \
         AND deadline_at > clock_timestamp()";

    /// 围栏条件的 SQL 模板（供断言与文档使用）。
    ///
    /// 返回的**就是** [`Fence::CONDITIONS_TEMPLATE`] —— 不是副本，不是重新拼的。
    pub fn conditions() -> &'static str {
        Self::CONDITIONS_TEMPLATE
    }

    /// 把围栏条件追加到查询构造器。
    ///
    /// # 绑定顺序
    ///
    /// `id` → `generation` → `lease_owner` —— 与模板里 `?` 的出现顺序一致。
    /// **不要改** —— 改了就与模板脱节。
    ///
    /// # 为什么值走 `push_bind` 而不是拼进文本
    ///
    /// `push_bind` 走**参数化**通道，值永远不会出现在 SQL 文本里。
    /// 拼进文本（哪怕 `format!`）都是「SQL 注入的形状」——
    /// 类型上就不该允许那种写法存在。
    ///
    /// # 为什么不用 `format!` 替换 `?`
    ///
    /// 那会把值**变成 SQL 的一部分**。本实现让 sqlx 做编号与转义。
    pub fn push_conditions<'a>(
        &self,
        builder: &'a mut sqlx::QueryBuilder<sqlx::Postgres>,
    ) -> &'a mut sqlx::QueryBuilder<sqlx::Postgres> {
        // 按 `?` 切分模板：切出来的**奇数**段是绑定点之间的文本。
        //
        // 模板里恰好 3 个 `?` → 4 段。段数不对说明模板被改坏了
        // （加了或删了绑定点），`debug_assert` 会在开发期立刻发现。
        let mut segments = Self::CONDITIONS_TEMPLATE.split('?');

        builder.push(segments.next().unwrap_or_default());
        builder.push_bind(self.id.into_inner());
        builder.push(segments.next().unwrap_or_default());
        builder.push_bind(self.generation);
        builder.push(segments.next().unwrap_or_default());
        builder.push_bind(&self.lease_owner);
        builder.push(segments.next().unwrap_or_default());

        debug_assert!(
            segments.next().is_none(),
            "围栏条件模板必须恰好有 3 个绑定点（?）"
        );

        builder
    }

    /// 生成**完整**的围栏 `WHERE` 子句（**仅供测试与文档**）。
    ///
    /// # 用途
    ///
    /// 让测试能断言**真实生成**的 SQL，而不是断言一份重复的常量。
    /// 变异测试的教训：「断言一份副本」测不出「实现是否还对」。
    ///
    /// 生成的 SQL 形如：
    ///
    /// ```text
    /// WHERE id = $1 AND generation = $2 AND lease_owner = $3 AND status = 'running' ...
    /// ```
    pub fn where_sql(&self) -> String {
        let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new("WHERE ");
        self.push_conditions(&mut builder);
        builder.sql().as_str().to_string()
    }
}

/// 持有租约的运行句柄。
///
/// # 不变量
///
/// - **持有 `generation`** —— 调用方拿不到裸数字。
/// - **不可伪造** —— 字段私有，只有 [`crate::leases::ExecutionLeaseRepository::claim`]
///   能构造（`new` 是 `pub(crate)`）。
/// - **`run_id` 是 `RunId`**（NEX-4 的类型化 ID），不是裸 `i64`。
///
/// # 租约与围栏的关系
///
/// `RunLease` 可以**降级**成 [`Fence`]（[`RunLease::fence`]）——
/// 那是给「需要把围栏传进通用 SQL 辅助函数」的场景用的。
///
/// 但**写入方法**接受 `&RunLease` 而不是 `&Fence` —— 因为
/// `RunLease` 才是「持有租约」的证据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLease {
    run_id: RunId,
    generation: i64,
    owner: String,
}

impl RunLease {
    /// 构造租约。
    ///
    /// ⚠️ 这是 **crate 内可见**的构造 —— 公开它就等于允许伪造租约。
    ///
    /// # 为什么不让调用方直接构造
    ///
    /// 如果 `RunLease::new` 是 `pub`，调用方可以随手写
    /// `RunLease::new(run_id, 0, "worker")` —— 一个**从未领取过**的租约。
    /// 它的 `generation = 0` 会匹配到还没被任何人领取过的 run，
    /// 于是「fencing」形同虚设。
    ///
    /// 正确路径只有一条：**先 [`crate::leases::ExecutionLeaseRepository::claim`] 领取**。
    pub(crate) fn new(run_id: RunId, generation: i64, owner: impl Into<String>) -> Self {
        Self {
            run_id,
            generation,
            owner: owner.into(),
        }
    }

    /// 目标 run 的 ID。
    ///
    /// 这是**唯一**可以公开取出的字段 —— 它不是围栏的一部分，
    /// 拿它去写别的表是合法的。
    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    /// 租约持有者标识（便于日志与排障）。
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// 降级成 [`Fence`]。
    ///
    /// # 什么时候用
    ///
    /// 当围栏条件需要被**传进**一个通用 SQL 辅助函数时。
    /// 正常路径上的写入方法应该直接收 `&RunLease`。
    pub fn fence(&self) -> Fence {
        Fence {
            id: self.run_id,
            generation: self.generation,
            lease_owner: self.owner.clone(),
        }
    }
}

/// 围栏写入的结果。
///
/// # 为什么用枚举而不是 `bool`
///
/// `bool` 不能区分「没写成功」与「被 fencing 拒绝」——
/// 而这两者的**处置完全不同**：
///
/// - 没写成功 → 重试（可能是瞬时冲突）
/// - 被 fencing 拒绝 → **本 worker 已经是僵尸**，必须停止一切写入并退出
///
/// 这正是「失败是设计的一部分」（§1 原则 4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseOutcome {
    /// 写入成功（影响 1 行）。
    Applied,
    /// **被 fencing 拒绝**（影响 0 行）—— 本 worker 已是僵尸。
    ///
    /// 遇到这个结果，调用方**必须停止**对该 run 的后续写入。
    Fenced,
}

impl LeaseOutcome {
    /// 写入是否成功。
    pub fn is_applied(self) -> bool {
        matches!(self, Self::Applied)
    }

    /// 是否被 fencing 拒绝。
    pub fn is_fenced(self) -> bool {
        matches!(self, Self::Fenced)
    }
}

/// 「租约已丢失」的判定结果。
///
/// # 为什么单独一个类型
///
/// fencing 被拒绝时，调用方需要区分「租约丢了」与「数据库出问题了」。
/// 前者要**停止工作**，后者要**重试**。
#[derive(Debug, Clone)]
pub struct LeaseLost {
    run_id: RunId,
}

impl LeaseLost {
    /// 构造。
    pub fn new(run_id: RunId) -> Self {
        Self { run_id }
    }

    /// 丢失租约的 run ID。
    pub fn run_id(&self) -> RunId {
        self.run_id
    }
}

impl std::fmt::Display for LeaseLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "run {} 的租约已丢失（被 fencing 拒绝）", self.run_id)
    }
}

impl std::error::Error for LeaseLost {}

impl From<LeaseLost> for AppError {
    fn from(lost: LeaseLost) -> Self {
        // `RunConflict`（409，终止型）—— 「租约丢了」是**明确失败**：
        // 本 worker 已被取代，重试没有意义，必须停止。
        AppError::new(
            ErrorCode::RunConflict,
            "执行租约已失效，请停止对该运行的写入",
        )
        .with_cause_msg(format!("{}", lost))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_list_all_six_clauses_in_bind_order() {
        let fence = Fence::new(RunId::new(7), 3, "worker-a");

        // ⚠️ 断言的是**真实生成的 SQL**，不是一份重复的常量。
        //
        // 变异测试的教训：断言副本测不出「实现是否还对」——
        // 把 `push_conditions` 里的 `generation` 条件删掉，
        // 断言副本的测试照样绿。
        let sql = fence.where_sql();

        // 顺序必须与 `push_conditions` 的绑定顺序一致。
        let p1 = sql.find("$1").expect("应有 $1");
        let p2 = sql.find("$2").expect("应有 $2");
        let p3 = sql.find("$3").expect("应有 $3");
        assert!(p1 < p2 && p2 < p3, "占位符顺序应递增：{sql}");

        for cond in [
            "id = $1",
            "generation = $2",
            "lease_owner = $3",
            "status = 'running'",
            "lease_until > clock_timestamp()",
            "deadline_at > clock_timestamp()",
        ] {
            assert!(sql.contains(cond), "围栏条件缺少 {cond}：{sql}");
        }
    }

    #[test]
    fn generated_sql_has_no_adjacent_or_stray_placeholders() {
        // ⚠️ 这条断言防的是**真实发生过**的一个缺陷：
        //
        // 早期实现用 `push_bind`（自动编号）拼一个**硬编码 `$1/$2/$3`**
        // 的常量片段，生成出 `WHERE $1$2$3 AND id = $1 AND ...` ——
        // 语法错误，且**只有真连上数据库才会暴露**。
        //
        // 现在模板用 `?` 标记绑定点，由 sqlx 编号，生成的一定是
        // `WHERE id = $1 AND generation = $2 AND lease_owner = $3 AND ...`。
        let fence = Fence::new(RunId::new(7), 3, "worker-a");
        let sql = fence.where_sql();

        assert!(
            !sql.contains("$1$2"),
            "占位符不该相邻（自动编号与硬编码混用的症状）：{sql}"
        );
        assert!(
            !sql.contains("$2$3"),
            "占位符不该相邻（自动编号与硬编码混用的症状）：{sql}"
        );
        // 每个占位符**恰好**出现一次。
        for placeholder in ["$1", "$2", "$3"] {
            let count = sql.matches(placeholder).count();
            assert_eq!(count, 1, "{placeholder} 应恰好出现一次：{sql}");
        }
        // 模板的 `?` 不应残留在 SQL 里。
        assert!(!sql.contains('?'), "绑定点标记不该出现在 SQL 里：{sql}");
    }

    #[test]
    fn conditions_use_clock_timestamp_not_now() {
        let fence = Fence::new(RunId::new(7), 3, "worker-a");
        let sql = fence.where_sql();

        // `now()` 在事务内是固定值 → 租约过期判定会错。
        assert!(
            sql.contains("clock_timestamp()"),
            "必须用 clock_timestamp()：{sql}"
        );
        assert!(!sql.contains("now()"), "不能出现 now()：{sql}");
    }

    #[test]
    fn generated_where_clause_does_not_inline_the_values() {
        // 围栏的值必须走**参数化**通道 —— 不能拼进 SQL 文本。
        //
        // 「拼进 SQL」是注入的形状。哪怕这里的值来自内部，
        // 也不该让「值出现在 SQL 文本里」成为可能。
        let fence = Fence::new(RunId::new(7), 3, "worker-a");
        let sql = fence.where_sql();

        assert!(!sql.contains("worker-a"), "持有者标识不应被拼进 SQL：{sql}");
        assert!(!sql.contains("= 7"), "run ID 不应被拼进 SQL：{sql}");
    }

    #[test]
    fn template_has_exactly_three_binding_points() {
        // 模板的绑定点数必须与 `push_conditions` 绑定的值数一致。
        // 多一个少一个 → 绑定错位 → fencing 静默失效。
        let count = Fence::conditions().matches('?').count();

        assert_eq!(
            count,
            3,
            "围栏模板应恰好有 3 个绑定点：{}",
            Fence::conditions()
        );
    }

    #[test]
    fn lease_derives_fence_with_the_same_generation() {
        let lease = RunLease::new(RunId::new(7), 3, "worker-a");
        let fence = lease.fence();

        assert_eq!(fence.id, RunId::new(7));
        assert_eq!(fence.generation, 3);
        assert_eq!(fence.lease_owner, "worker-a");
    }

    #[test]
    fn lease_exposes_run_id_but_not_generation() {
        let lease = RunLease::new(RunId::new(7), 3, "worker-a");

        assert_eq!(lease.run_id(), RunId::new(7));
        assert_eq!(lease.owner(), "worker-a");

        // ⚠️ 「不提供返回裸 generation 的公开方法」这条约束，
        // **由 `tests/compile_fail.rs` 的 trybuild 负向测试保证** ——
        // 那才是类型级的证明（编译不过）。
        //
        // 这里**不做**源码字符串扫描 —— 那种断言会匹配到它自己的字面量，
        // 而且它测的是「文本」而不是「API」。
    }

    #[test]
    fn lease_lost_maps_to_run_conflict() {
        let err: AppError = LeaseLost::new(RunId::new(9)).into();

        assert_eq!(err.user_facing().code, ErrorCode::RunConflict);
        // 终止型 —— 租约丢了重试没有意义。
        assert_eq!(err.retry_policy(), nexus_core::error::RetryPolicy::Terminal);
    }

    #[test]
    fn lease_outcome_distinguishes_applied_from_fenced() {
        assert!(LeaseOutcome::Applied.is_applied());
        assert!(!LeaseOutcome::Applied.is_fenced());
        assert!(LeaseOutcome::Fenced.is_fenced());
        assert!(!LeaseOutcome::Fenced.is_applied());
    }
}
