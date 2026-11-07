//! **token 级**扫描器的**对抗输入**测试。
//!
//! # 为什么扫描器自己要有测试
//!
//! 扫描器是**结构层断言的底层工具**。它坏掉时：
//! 上层断言**照样绿**（因为它们只看扫描器的输出），
//! 而真正的违规**已经漏掉了**。
//!
//! 这正是本仓库反复付出代价的失效形态 ——
//! 「看起来覆盖了，实际没有」。
//! 所以**扫描器本身**必须有**对抗输入**的测试，
//! 证明它对每一种已知的失效形态都**不**失效。
//!
//! # 四组对抗输入
//!
//! | # | 输入 | 旧手写词法器的失效形态 |
//! |---|---|---|
//! | 1 | `&'static` **之后**的注释 | api 副本：生命周期 `'` 只开不关 → 注释**漏剥** |
//! | 2 | 字符串 `"http://x"` 后的真实代码 | agent 副本：无字符串状态 → 该行**之后**的真实代码**被丢弃** |
//! | 3 | 字符字面量 `'"'` 之后的注释 | core 副本：无字符状态 → 注释**漏剥** |
//! | 4 | 字符串内含 `/*` | 任何把它当块注释开始的实现 |

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
#[path = "support/token_scan.rs"]
mod token_scan;

/// 标识符出现在**注释**里 → **不应**被收集。
#[test]
fn a_comment_is_not_scanned_as_code() {
    let source = r#"
//! 本 crate 不含 FromRow。
use serde::Serialize;
/// 行注释里的 sqlx:: 也要剥掉。
/* 块注释里的 FromRow 同样剥掉。 */
pub struct Foo;
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !result.identifiers.contains("FromRow"),
        "注释里的 `FromRow` 是说明文字，不是数据库痕迹：{:?}",
        result.identifiers
    );
    assert!(
        !result.identifiers.contains("sqlx"),
        "注释里的 `sqlx` 不应被收集：{:?}",
        result.identifiers
    );
    // ⚠️ 代码里的标识符**必须**还在 —— 防止「把代码也剥掉了」。
    assert!(
        result.identifiers.contains("Serialize"),
        "代码被误剥：{:?}",
        result.identifiers
    );
    assert!(
        result.identifiers.contains("Foo"),
        "代码被误剥：{:?}",
        result.identifiers
    );
}

/// **对抗输入 1**：`&'static` **之后**的注释必须被正确剥离。
///
/// ⚠️ 这是 api 副本今天的失败点 ——
/// 它的字符状态机把生命周期 `'` 当成字符字面量的开始，且**只开不关**，
/// 于是从这个 `'` 之后几乎**全部**文本都不再被剥注释。
#[test]
fn a_lifetime_does_not_open_a_string_state() {
    let source = r#"
fn f() -> &'static str {
    let s = "x";
    s
}
// FORBIDDEN_MARKER
fn g() {}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !result.identifiers.contains("FORBIDDEN_MARKER"),
        "`&'static` 之后的注释必须被剥离：{:?}",
        result.identifiers
    );
    // ⚠️ 代码**必须**还在 —— 这才是这条测试的意义。
    assert!(
        result.identifiers.contains("g"),
        "生命周期之后的代码被误剥：{:?}",
        result.identifiers
    );
}

/// **对抗输入 2**：字符串里的 `//` **不是**注释，其后的真实代码**必须**保留。
///
/// ⚠️ 这是 agent 副本今天的失败点 —— 它**完全没有字符串状态**，
/// 于是 `"http://x"` 里的 `//` 启动了行注释，把**该行之后**的代码全吞了。
#[test]
fn a_slash_inside_a_string_is_not_a_comment() {
    let source = r#"
fn f() {
    let u = "http://example.com";
    REAL_VIOLATION_MARKER();
}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        result.identifiers.contains("REAL_VIOLATION_MARKER"),
        "字符串里的 `//` 不是注释 —— 其后的真实代码必须保留：{:?}",
        result.identifiers
    );
    // ⚠️ 反过来也要断言：字符串**本身**的内容**不该**出现在标识符集里。
    assert!(
        !result.identifiers.contains("example"),
        "字符串内容不是标识符：{:?}",
        result.identifiers
    );
}

/// **对抗输入 3**：字符字面量 `'"'` **之后**的注释必须被正确剥离。
///
/// ⚠️ 这是 core 副本今天的失败点 —— 它**没有字符字面量状态**，
/// 于是 `'"'` 里的 `'` 打开了字符串状态，其后的注释**不再被剥**。
#[test]
fn a_char_literal_does_not_open_a_string_state() {
    let source = r#"
fn f() {
    let c = '"';
    let d = '\'';
}
// FORBIDDEN_MARKER
fn g() {}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !result.identifiers.contains("FORBIDDEN_MARKER"),
        "字符字面量之后的注释必须被剥离：{:?}",
        result.identifiers
    );
    assert!(
        result.identifiers.contains("g"),
        "字符字面量之后的代码被误剥：{:?}",
        result.identifiers
    );
}

/// **对抗输入 4**：字符串内含 `/*` **不**是块注释的开始。
#[test]
fn a_block_comment_opener_inside_a_string_is_not_a_comment() {
    let source = r#"
fn f() {
    let s = "/* not a comment */";
    ANOTHER_REAL_MARKER();
}
// FORBIDDEN_MARKER
fn g() {}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !result.identifiers.contains("FORBIDDEN_MARKER"),
        "字符串里的 `/*` 不得开启块注释：{:?}",
        result.identifiers
    );
    assert!(
        result.identifiers.contains("ANOTHER_REAL_MARKER"),
        "字符串里的 `/*` 之后的代码必须保留：{:?}",
        result.identifiers
    );
}

/// **原始字符串**（`r#"..."#`）里的内容同样不是代码。
#[test]
fn a_raw_string_is_scanned_as_a_string_not_as_code() {
    let source = r##"
fn f() {
    let s = r#"FORBIDDEN_MARKER"#;
    REAL_MARKER();
}
"##;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !result.identifiers.contains("FORBIDDEN_MARKER"),
        "原始字符串的内容不是标识符：{:?}",
        result.identifiers
    );
    assert!(
        result.identifiers.contains("REAL_MARKER"),
        "原始字符串之后的代码必须保留：{:?}",
        result.identifiers
    );
}

/// 嵌套块注释 —— Rust **允许**嵌套。
#[test]
fn a_nested_block_comment_is_stripped() {
    let source = r#"
/* outer /* inner */ FORBIDDEN_MARKER */
fn g() {}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !result.identifiers.contains("FORBIDDEN_MARKER"),
        "嵌套块注释必须被完整剥离：{:?}",
        result.identifiers
    );
    assert!(result.identifiers.contains("g"), "代码被误剥");
}

/// **语义边界**：字符串字面量在两个视图里的**不同**待遇。
///
/// 这是本模块最容易被误用的地方，所以必须有测试钉住。
#[test]
fn the_two_views_differ_on_string_literals() {
    let source = r#"
fn f() {
    let s = "IDENTIFIER_INSIDE_A_STRING";
}
"#;
    let code_only = token_scan::scan_code(source).expect("解析失败");
    assert!(
        !code_only.identifiers.contains("IDENTIFIER_INSIDE_A_STRING"),
        "`code_only` 视图不含字符串字面量"
    );

    let with_strings = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        with_strings.strings.contains("IDENTIFIER_INSIDE_A_STRING"),
        "`code_and_strings` 视图必须含字符串字面量"
    );
}

/// **文档注释**里的内容**不是**代码。
///
/// ⚠️ 这条抓的是一个**真实的**坑：`syn` 把 `///` 与 `//!` 解析成
/// `#[doc = "..."]` **属性** —— 属性的值是一个 `LitStr`。
///
/// 若扫描器不跳过 `doc` 属性，**文档注释的内容就会混进字符串集合** ——
/// 那正是「注释里的 `workflow_runs` 被当成真实引用」的形态。
///
/// 实证：修好之前，`workflow_runs_appears_only_in_execution_owned_crates`
/// 报出 7 处「违规」，而那 7 处**全部**在文档注释里。
#[test]
fn a_doc_comment_is_not_scanned_as_a_string() {
    let source = r#"
//! `workflow_runs` 与 `workflow_step_runs` 由 `nexus-execution` 独占。
/// 架构文档 §4.2 差异 1：`workflow_runs` 由 `nexus-execution` 独占。
fn f() {}
"#;
    let result = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        !result.strings.iter().any(|s| s.contains("workflow_runs")),
        "文档注释不得进入字符串集合：{:?}",
        result.strings
    );
    assert!(
        !result.identifiers.contains("workflow_runs"),
        "文档注释不得进入标识符集合：{:?}",
        result.identifiers
    );
}

/// **普通字符串**里的内容**要**进入字符串集合（与文档注释相反）。
///
/// 这条钉住「跳过 `doc` 属性」**没有**过度跳过。
#[test]
fn a_regular_string_is_still_scanned() {
    let source = r#"
fn f() {
    let sql = "SELECT * FROM workflow_runs";
}
"#;
    let result = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        result.strings.iter().any(|s| s.contains("workflow_runs")),
        "普通字符串必须被扫描：{:?}",
        result.strings
    );
    assert!(
        result.mentions("workflow_runs"),
        "`mentions` 应当在字符串里找到子串"
    );
}

/// **完整路径**的判定与「词出现过」**不同**。
///
/// `axum` 与 `sse` 分别出现过**不等于** `axum::response::sse` 出现过。
#[test]
fn a_full_path_is_distinct_from_a_loose_word_match() {
    let source = r#"
use axum::Router;
fn f() {
    let sse = "sse";
}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(result.identifiers.contains("axum"));
    assert!(result.identifiers.contains("sse"));
    assert!(
        !result.has_path("axum::response::sse"),
        "`axum` 与 `sse` 分别出现**不等于**完整路径存在：{:?}",
        result.paths
    );
    assert!(
        result.has_path("axum::Router"),
        "完整路径应被记录：{:?}",
        result.paths
    );
}

/// **推进检测**只看**真正的赋值表达式**。
///
/// ⚠️ 抓的是旧实现（「逐行搜 `seq += `」）的两个问题：
/// 注释里的 `// seq += 1` 被算成一处推进（**假阳性**）。
#[test]
fn a_advancement_in_a_comment_is_not_counted() {
    let source = r#"
fn f() {
    let mut seq = 0u64;
    // seq += 1;  ← 这是注释，不是推进
    seq += 1;
}
"#;
    let advancements = token_scan::find_advancements(source, "seq").expect("解析失败");
    assert_eq!(
        advancements.len(),
        1,
        "只有真正的那一处推进才算：{advancements:?}"
    );
    assert_eq!(advancements[0].kind, "+=");
}

/// **函数调用**与「标识符出现过」**不同**。
#[test]
fn a_call_is_distinct_from_a_loose_word_match() {
    let source = r#"
struct Sleeper;
fn f() {
    let sleeper = Sleeper;
    sleeper.sleep();
}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(result.identifiers.contains("sleep"));
    assert!(
        result.calls.contains("sleep"),
        "函数调用应被记录：{:?}",
        result.calls
    );
}

/// **解析失败必须被报告** —— 不得静默跳过。
///
/// ⚠️ 这是「静默失效」的最后一个入口 ——
/// 一份语法错误的源文件如果被当成空文件扫，
/// 它上面的所有违规都会**消失**，而门禁**照样绿**。
#[test]
fn a_parse_error_is_reported_not_swallowed() {
    let broken = "fn f( { this is not rust";
    assert!(
        token_scan::scan_code(broken).is_err(),
        "语法错误必须让 `scan_code` 返回 Err"
    );
}

/// **SQL 写操作**的判定 —— 读与写**必须**分得开。
///
/// 这是架构文档 §4.2 差异 1（决策 D12 · 选项 B）的**判定基础**：
/// `workflow_runs` 的**写入权**归执行层独占。
#[test]
fn a_sql_write_is_distinguished_from_a_sql_read() {
    let source = r#"
fn f() {
    let read = "SELECT * FROM workflow_runs";
    let write = "INSERT INTO workflow_runs (id) VALUES ($1)";
}
"#;
    let result = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        result.writes_table("workflow_runs"),
        "INSERT 必须被识别为写操作：{:?}",
        result.sql_writes
    );
    assert!(!result.sql_writes.is_empty(), "写操作集合不应为空");
}

/// **只有 `SELECT` 的 SQL 不算写操作** —— 读不构成写入。
#[test]
fn a_select_only_statement_is_not_a_write() {
    let source = r#"
fn f() {
    let sql = "SELECT id FROM workflow_step_runs WHERE run_id = $1";
}
"#;
    let result = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        !result.writes_table("workflow_step_runs"),
        "SELECT 不是写操作：{:?}",
        result.sql_writes
    );
    // ⚠️ 但它**确实**提到了这张表 —— 证明「提到」与「写到」是两件事。
    assert!(
        result.mentions("workflow_step_runs"),
        "SELECT 仍然提到了表名：{:?}",
        result.strings
    );
}

/// **建表 DDL 不是写操作** —— `nexus-infra` 的迁移**必须**能通过。
///
/// 架构文档 §4.3.2 约束 2：`nexus-infra` 允许**建表 DDL**（迁移），不是访问。
#[test]
fn a_create_table_statement_is_not_a_write() {
    let source = r#"
fn f() {
    let ddl = "CREATE TABLE workflow_runs (id BIGINT PRIMARY KEY)";
}
"#;
    let result = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        !result.writes_table("workflow_runs"),
        "建表 DDL 不是写操作：{:?}",
        result.sql_writes
    );
}

/// **各种 SQL 形态都必须被识别** —— 大小写、换行、缩进、引号。
///
/// ⚠️ 这是「看起来覆盖了，实际没有」的高发区 ——
/// 只认 `INSERT INTO ` 一种形态的实现，会在别的写法上**静默漏报**。
#[test]
fn sql_write_survives_formatting_variations() {
    let cases = [
        (
            "INSERT INTO workflow_runs (id) VALUES ($1)",
            "workflow_runs",
        ),
        (
            "insert into workflow_runs (id) values ($1)",
            "workflow_runs",
        ),
        (
            "INSERT  INTO   workflow_runs (id) VALUES ($1)",
            "workflow_runs",
        ),
        (
            "INSERT\nINTO workflow_runs\n(id) VALUES ($1)",
            "workflow_runs",
        ),
        ("UPDATE workflow_runs SET status = 'done'", "workflow_runs"),
        ("update workflow_runs set status = 'done'", "workflow_runs"),
        (
            "DELETE FROM workflow_step_runs WHERE id = $1",
            "workflow_step_runs",
        ),
        (
            "delete from workflow_step_runs where id = $1",
            "workflow_step_runs",
        ),
        (
            r#"INSERT INTO "workflow_runs" (id) VALUES ($1)"#,
            "workflow_runs",
        ),
    ];

    for (sql, table) in cases {
        let writes = token_scan::extract_sql_writes(sql);
        assert!(
            writes.iter().any(|w| w.table == table),
            "未识别出 {table} 的写操作（SQL：{sql}）—— 实际：{writes:?}"
        );
    }
}

/// **字符串字面量里的内容不算写操作** —— 那是**值**，不是 SQL。
///
/// `WHERE name = 'update workflow_runs'` 里的 `update` 不是写操作。
/// 若不跳过字符串字面量，这类字面量会造成**假阳性**。
#[test]
fn a_write_inside_a_string_literal_is_not_a_write() {
    let source = r#"
fn f() {
    let sql = "SELECT * FROM runs WHERE name = 'INSERT INTO workflow_runs'";
}
"#;
    let result = token_scan::scan_code_and_strings(source).expect("解析失败");
    assert!(
        !result.writes_table("workflow_runs"),
        "字符串字面量里的 INSERT 不是写操作：{:?}",
        result.sql_writes
    );
}

/// **写操作的识别不依赖 `collect_strings`** —— 否则是绕过写权限的路径。
///
/// 「用 `code_only` 扫描」**不得**成为一条绕过写权限判定的路。
#[test]
fn sql_writes_are_detected_even_without_string_collection() {
    let source = r#"
fn f() {
    let sql = "INSERT INTO workflow_runs (id) VALUES ($1)";
}
"#;
    let result = token_scan::scan_code(source).expect("解析失败");
    assert!(
        result.writes_table("workflow_runs"),
        "写操作的判定必须独立于字符串收集：{:?}",
        result.sql_writes
    );
}

/// **目录扫描**必须把解析失败的文件**收集**起来。
#[test]
fn scan_dir_collects_parse_errors() {
    let dir = std::env::temp_dir().join("nexus_token_scan_parse_error_probe");
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    std::fs::write(dir.join("broken.rs"), "fn f( {").expect("写入失败");

    let result = token_scan::scan_dir(&dir, false);
    assert!(
        result.has_parse_errors(),
        "目录扫描必须收集解析失败的文件，而不是静默跳过"
    );
    assert_eq!(
        result.parse_errors.len(),
        1,
        "应当恰好报告一个解析失败：{:?}",
        result.parse_errors
    );

    std::fs::remove_dir_all(&dir).ok();
}
