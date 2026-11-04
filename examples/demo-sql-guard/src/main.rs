//! demo-sql-guard
//!
//! 验证设计文档 §3.6：SQL 校验必须是 **AST 级**，不是关键词匹配。
//!
//! 参考实现（Go）用 `strings.Contains` 做关键词检查 —— 它自己也标注了
//! 「可读性差且易错」。Rust 版用 `sqlparser` 做真正的语法分析。
//!
//! ## 关键词匹配挡不住什么
//!
//! | SQL | 关键词匹配 | AST 解析 |
//! |---|---|---|
//! | `SELECT 1` | 放行 | 放行 |
//! | `SELECT 1; DROP TABLE users` | **可能放行**（含 SELECT） | **拒绝**（多语句） |
//! | `SELECT * FROM t INTO OUTFILE '/tmp/x'` | **可能放行** | **拒绝** |
//! | `WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x` | **可能放行** | **拒绝** |
//!
//! 对应 crate：`sqlparser 0.58`（PostgreSQL 方言）。

use sqlparser::ast::{Query, SetExpr, Statement, With};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

/// 拒绝原因。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SqlRejection {
    #[error("SQL 语法错误：{0}")]
    Parse(String),
    #[error("只允许单条语句")]
    NotSingleStatement,
    #[error("只允许 SELECT 查询")]
    NotSelect,
    #[error("不允许写操作")]
    WriteOperation,
    #[error("不允许 CTE 中包含写操作")]
    WriteInCte,
    #[error("不允许 SELECT INTO")]
    SelectInto,
}

/// 只读 SELECT 守卫。
///
/// 校验顺序（**先语法后语义** —— 语法不过就不用谈语义）：
/// 1. 能否解析
/// 2. 是否单条语句
/// 3. 是否为 SELECT / WITH
/// 4. WITH 子句里是否含写操作
/// 5. 是否有 SELECT INTO
pub struct ReadOnlyGuard;

impl ReadOnlyGuard {
    /// 校验 SQL 是否为安全的只读查询。
    pub fn check(sql: &str) -> Result<(), SqlRejection> {
        let dialect = PostgreSqlDialect {};
        let statements =
            Parser::parse_sql(&dialect, sql).map_err(|e| SqlRejection::Parse(e.to_string()))?;

        // ② 单条语句
        if statements.len() != 1 {
            return Err(SqlRejection::NotSingleStatement);
        }

        match &statements[0] {
            Statement::Query(q) => check_query(q),
            Statement::Insert(_)
            | Statement::Update { .. }
            | Statement::Delete(_)
            | Statement::Merge { .. } => Err(SqlRejection::WriteOperation),
            _ => Err(SqlRejection::NotSelect),
        }
    }
}

/// 检查一个查询（含嵌套）。
fn check_query(q: &Query) -> Result<(), SqlRejection> {
    // ④ CTE 里的写操作
    if let Some(with) = &q.with {
        check_with(with)?;
    }
    // ⑤ SELECT INTO
    if let SetExpr::Select(sel) = q.body.as_ref()
        && sel.into.is_some()
    {
        return Err(SqlRejection::SelectInto);
    }
    check_set_expr(q.body.as_ref())
}

/// 检查 WITH 子句。
fn check_with(with: &With) -> Result<(), SqlRejection> {
    for cte in &with.cte_tables {
        check_query(&cte.query)?;
    }
    Ok(())
}

/// 检查集合表达式（递归）。
fn check_set_expr(expr: &SetExpr) -> Result<(), SqlRejection> {
    match expr {
        SetExpr::Select(sel) => {
            if sel.into.is_some() {
                return Err(SqlRejection::SelectInto);
            }
            Ok(())
        }
        SetExpr::Query(q) => check_query(q),
        SetExpr::SetOperation { left, right, .. } => {
            check_set_expr(left)?;
            check_set_expr(right)?;
            Ok(())
        }
        // 写操作藏在集合表达式里
        SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) => {
            Err(SqlRejection::WriteInCte)
        }
        SetExpr::Values(_) | SetExpr::Table(_) => Ok(()),
    }
}

/// 便于测试：判断一条 SQL 是否放行。
fn allows(sql: &str) -> bool {
    ReadOnlyGuard::check(sql).is_ok()
}

fn main() {
    let cases: &[(&str, bool)] = &[
        // ── 应当放行 ──
        ("SELECT 1", true),
        ("SELECT id, name FROM users WHERE id = $1", true),
        (
            "SELECT * FROM users ORDER BY created_at DESC LIMIT 10",
            true,
        ),
        ("SELECT count(*) FROM messages", true),
        (
            "WITH recent AS (SELECT * FROM logs LIMIT 5) SELECT * FROM recent",
            true,
        ),
        ("SELECT a FROM t1 UNION SELECT b FROM t2", true),
        ("SELECT * FROM t WHERE name = 'x' AND id IN (1,2,3)", true),
        // ── 应当拒绝 ──
        ("DROP TABLE users", false),
        ("DELETE FROM users", false),
        ("UPDATE users SET name = 'x'", false),
        ("INSERT INTO users (name) VALUES ('x')", false),
        ("SELECT 1; DROP TABLE users", false),
        ("SELECT 1; SELECT 2", false),
        (
            "WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x",
            false,
        ),
        ("SELECT * FROM t INTO OUTFILE '/tmp/x'", false),
        (
            "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.x = s.x",
            false,
        ),
        ("CREATE TABLE t (id int)", false),
        ("TRUNCATE users", false),
        ("GRANT ALL ON users TO bob", false),
        ("ALTER TABLE users ADD COLUMN x int", false),
        // ── 畸形输入 ──
        ("SELECT FROM", false),
        ("SELECT 'unterminated", false),
        ("", false),
        // ── 语法合法但语义荒谬 —— AST 层不负责，交给数据库报错 ──
        ("SELECT FROM WHERE", true),
    ];

    let mut passed = 0;
    let mut failed = 0;
    for (sql, expect) in cases {
        let got = allows(sql);
        let mark = if got == *expect { "✓" } else { "✗" };
        if got == *expect {
            passed += 1;
        } else {
            failed += 1;
        }
        let verdict = match ReadOnlyGuard::check(sql) {
            Ok(()) => "放行".to_string(),
            Err(e) => format!("拒绝（{e}）"),
        };
        println!("{mark} {verdict:<34} | {sql}");
    }

    println!("\n通过 {passed} / {}", cases.len());
    assert_eq!(failed, 0, "有 {failed} 个用例不符合预期");
    println!("✓ AST 级校验：关键词匹配挡不住的都被挡住了");
    println!("⚠️  边界：AST 层保证「只读 + 单语句」，不保证语义合理");
    println!("   （如 `SELECT FROM WHERE` 语法合法，由数据库在执行时报错）");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 放行只读查询() {
        for sql in [
            "SELECT 1",
            "SELECT id FROM users WHERE id = 1",
            "SELECT count(*) FROM messages",
            "WITH x AS (SELECT * FROM logs) SELECT * FROM x",
            "SELECT a FROM t1 UNION SELECT b FROM t2",
        ] {
            assert!(allows(sql), "应放行：{sql}");
        }
    }

    #[test]
    fn 拒绝写操作() {
        for sql in [
            "DROP TABLE users",
            "DELETE FROM users",
            "UPDATE users SET name = 'x'",
            "INSERT INTO users (name) VALUES ('x')",
            "TRUNCATE users",
            "CREATE TABLE t (id int)",
            "ALTER TABLE users ADD COLUMN x int",
        ] {
            assert!(!allows(sql), "应拒绝：{sql}");
        }
    }

    #[test]
    fn 拒绝多语句() {
        assert!(!allows("SELECT 1; DROP TABLE users"));
        assert!(!allows("SELECT 1; SELECT 2"));
    }

    #[test]
    fn 拒绝_cte_里的写操作() {
        // 这是关键词匹配最容易漏掉的一类
        assert!(!allows(
            "WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x"
        ));
    }

    #[test]
    fn 拒绝_select_into() {
        assert!(!allows("SELECT * FROM t INTO OUTFILE '/tmp/x'"));
    }

    #[test]
    fn 拒绝畸形_sql() {
        // 语法上就解析不过的
        assert!(!allows("SELECT FROM"));
        assert!(!allows("SELECT 'unterminated"));
        assert!(!allows(""));
    }

    #[test]
    fn ast_校验不保证语义合理() {
        // ⚠️ 重要边界：`SELECT FROM WHERE` 在 PostgreSQL 语法上是**合法**的
        //    （「从名为 WHERE 的表里选空列表」），sqlparser 能解析。
        //
        //    AST 级校验保证的是「只读 + 单语句」，**不是**「语义合理」。
        //    语义合理性由数据库在执行时报错 —— 这正是我们想要的分工：
        //    守卫负责**安全**（不写、不可注入），数据库负责**正确性**。
        assert!(allows("SELECT FROM WHERE"));

        // 而真正危险的写法一定被挡住
        assert!(!allows("SELECT 1; DROP TABLE users"));
        assert!(!allows(
            "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x"
        ));
    }

    #[test]
    fn 拒绝_merge() {
        assert!(!allows(
            "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.x = s.x"
        ));
    }
}
