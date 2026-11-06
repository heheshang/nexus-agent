//! **token 级**源码扫描 —— 结构层断言的共用底层工具。
//!
//! # 为什么不用「手写词法器剥注释」
//!
//! 本仓库曾有三份**手抄**的「剥离注释」实现
//! （`nexus-api` / `nexus-agent` / `nexus-core` 各一份）。
//! 它们**行为不一致**，且每一份都有各自的失效方式：
//!
//! | 副本 | 失效方式 | 后果 |
//! |---|---|---|
//! | api | `&'static` 的生命周期 `'` **只开不关** | 该点之后几乎全部文本不再被剥 → **假阳性** |
//! | agent | **没有字符串状态** | `"http://x"` 里的 `//` 被当成行注释 → **该行之后的真实代码被丢弃**（**真漏报**） |
//! | core | **没有字符字面量状态** | 字符字面量 `'"'` 会**打开**字符串状态 → 其后的注释不再被剥 → **假阳性** |
//!
//! 更要紧的是：**这三份是结构层断言的底层工具**。
//! 底层扫描器漏报，**上层断言全部失效**，而门禁**照样绿** ——
//! 那正是本仓库反复出现、也反复付出代价的失效形态
//! （「看起来覆盖了，实际没有」）。
//!
//! # 本模块的做法：token 级，而不是文本级
//!
//! 用 `syn` 把源码解析成 **AST**，只遍历 **token / 节点**，
//! **从不**在原始文本上做正则匹配。
//!
//! 这样做的直接后果：
//!
//! - **注释**根本不是 token —— 它在 `syn` 的 AST 里**不存在**，
//!   所以「剥离注释」这件事**自动成立**，不需要词法器去「猜」哪里是注释；
//! - **字符串字面量**是 `syn::LitStr` 节点 —— 它**不是**标识符，
//!   所以「字符串里的 `//` 不是注释」也**自动成立**；
//! - **字符字面量**同理；
//! - **生命周期**（`'static`）在 `syn` 里是 `Lifetime` 节点 ——
//!   不会与字符字面量混淆。
//!
//! # ⚠️ 语义边界：字符串字面量**算不算**「代码」
//!
//! 本模块提供两种视图，调用方**必须显式选择**：
//!
//! | 视图 | 含字符串字面量 | 用途 |
//! |---|---|---|
//! | [`code_only`] | ❌ 不含 | 「源码里有没有用某个标识符」 |
//! | [`code_and_strings`] | ✅ 含 | 「源码的**任何地方**有没有提到某个字面量」 |
//!
//! **为什么必须区分**：结构层断言有两类需求。
//!
//! - 「`nexus-model` 里有没有 `FromRow`」—— 这是**标识符**问题 → 用 `code_only`。
//! - 「`nexus-api` 里有没有 `axum::response::sse`」—— 这是**路径**问题 → 用 `code_only`。
//! - 「`nexus-repo` 里有没有 `workflow_runs`」—— 这**既**可能是表名标识符，
//!   **也**可能是 SQL 字符串里的表名 → 用 `code_and_strings`（更严）。
//!
//! 选错方向的代价是**静默失效**：用 `code_only` 扫 `workflow_runs` 会漏掉
//! `"SELECT * FROM workflow_runs"` 这样的 SQL —— 而那**正是**要防的。
//!
//! # 解析失败怎么办
//!
//! [`parse`] 遇到**语法错误**的源码时返回 `Err` —— 调用方**必须**显式处理。
//! 绝**不**能「解析失败就当空文件扫」—— 那会让一份坏掉的源文件
//! 悄悄通过所有结构层断言（又一次「看起来覆盖了，实际没有」）。

// ⚠️ 这个模块被**多个** crate 的测试 `#[path]` include。
//
// 不同的调用方用到**不同**的函数 —— 于是每个调用方都会看到
// 「其余函数未使用」的告警。
//
// `#![allow(dead_code)]` 在这里**是**正确的：
// 这是一个**共用**的支撑模块，**不是**某个 crate 的私有实现，
// 「未被使用」是**正常**状态 —— 它取决于哪个 crate 在用它。
//
// ⚠️ 这条 `allow` 只覆盖 `dead_code` —— 它**不**放宽任何别的 lint。
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// 扫描源码的**结果**。
///
/// # 为什么用 `BTreeSet` 而不是 `Vec`
///
/// 扫描的用途是「**有没有**」与「**哪些**」——
/// 集合天然去重、顺序稳定，断言的失败消息也稳定可复现。
#[derive(Debug, Default, Clone)]
pub struct ScanResult {
    /// 匹配到的**标识符**（按字典序）。
    pub identifiers: BTreeSet<String>,
    /// 匹配到的**完整路径**（如 `axum::response::sse`，按字典序）。
    ///
    /// # 为什么**路径**要单独收集
    ///
    /// 「有没有用 axum 的 `Sse` 类型」这个判定的**原始形态**是
    /// 「有没有出现 `axum::response::sse`」—— 一条**完整路径**。
    /// 只收集**路径段**的话，判定会退化成「`axum` 和 `sse` 都出现过」，
    /// 那**太宽松**：任何文件只要**分别**提到过这两个词就会误报。
    ///
    /// 收集完整路径让判定**保持原来的精确度**，同时不依赖文本形态。
    pub paths: BTreeSet<String>,
    /// 被**调用**的函数名（按字典序）。
    ///
    /// # 为什么需要**单独**收集调用
    ///
    /// 「有没有用 `sleep(`」这类判定的**原始形态**是**函数调用**，
    /// 而**不是**「源码里出现过 `sleep` 这个词」。
    ///
    /// 两者的区别很实际：`sleep` 作为**标识符**出现（变量名、字段名、
    /// 文档里的词）不等于**调用** `sleep`。
    /// 只看标识符会把「定义了名为 `sleep` 的结构体」也判成违规。
    pub calls: BTreeSet<String>,
    /// 匹配到的**字符串字面量**（按字典序）。
    pub strings: BTreeSet<String>,
    /// 解析失败的文件（路径 → 错误信息）。
    pub parse_errors: Vec<(PathBuf, String)>,
    /// 源码里的 **SQL 写操作**（按字典序）。
    ///
    /// # 为什么需要**单独**收集
    ///
    /// 「某个 crate 能不能写某张表」这个判定的**原始形态**是
    /// 「这段 SQL 是不是 `INSERT` / `UPDATE` / `DELETE` 那张表」。
    ///
    /// 只看标识符或字符串，判不出「这句话**是**写操作」——
    /// 「`SELECT * FROM workflow_runs`」与
    /// 「`INSERT INTO workflow_runs ...`」**都**含表名，
    /// 但前者是**读**，后者是**写**。
    ///
    /// 架构文档 §4.2 差异 1（决策 D12 · 选项 B）要求的正是这个区分：
    /// `workflow_runs` 的**写操作**只允许出现在 `nexus-execution`。
    pub sql_writes: BTreeSet<SqlWrite>,
}

/// 一条 **SQL 写操作**。
///
/// # 字段为什么这么少
///
/// 只保留**能判定归属**的信息：操作种类与**目标表**。
/// 判定「这个 crate 能不能写这张表」只需要这两样。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SqlWrite {
    /// 操作种类（`INSERT` / `UPDATE` / `DELETE`）。
    pub operation: String,
    /// 目标表名。
    pub table: String,
}

impl ScanResult {
    /// 是否**曾经**解析失败。
    ///
    /// 调用方**应该**在断言前检查它 —— 一份解析不了的源文件
    /// 会让扫描结果**不完整**，而那正是「静默失效」的形态。
    pub fn has_parse_errors(&self) -> bool {
        !self.parse_errors.is_empty()
    }

    /// 源码里是否**提到**过某个词。
    ///
    /// # ⚠️ 两种匹配语义**不同**，这是**故意**的
    ///
    /// | 来源 | 匹配方式 | 为什么 |
    /// |---|---|---|
    /// | **标识符** | **精确**相等 | `sqlx` 这个标识符出现了 —— 精确匹配，���免「`sqlx_helper`」误命中 |
    /// | **字符串字面量** | **子串**包含 | `"SELECT * FROM workflow_runs"` 里的表名是**子串**，不是整个字符串 |
    ///
    /// 标识符用精确匹配、字符串用子串匹配 —— 因为它们的**形态**不同：
    /// 标识符是**词**，字符串是**任意文本**。
    ///
    /// # 参数
    ///
    /// `term` —— 要找的词。
    pub fn mentions(&self, term: &str) -> bool {
        self.identifiers.contains(term) || self.strings.iter().any(|s| s.contains(term))
    }

    /// 源码里是否**出现过**某条**完整路径**。
    ///
    /// 与 [`ScanResult::mentions`] 的区别：这里**只**看路径，
    /// 且是**完整**匹配（`axum::response::sse`）——
    /// 「`axum` 和 `sse` 都出现过」**不算**。
    pub fn has_path(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    /// 源码里是否**写**过某张表。
    ///
    /// # 与 [`ScanResult::mentions`] 的区别
    ///
    /// `mentions` 判的是「**提到**过」—— 读与写**不分**。
    /// 本方法判的是「**写**过」—— 只有 `INSERT` / `UPDATE` / `DELETE`
    /// 才算。
    ///
    /// 架构文档 §4.2 差异 1 判定的正是后者：
    /// `workflow_runs` 的**写入权**归执行层独占。
    pub fn writes_table(&self, table: &str) -> bool {
        self.sql_writes
            .iter()
            .any(|w| w.table.eq_ignore_ascii_case(table))
    }
}

/// 解析一份 Rust 源码。
///
/// # 错误
///
/// 源码有**语法错误**时返回 `Err` —— 调用方**必须**显式处理，
/// 不得静默跳过（见模块文档的「解析失败怎么办」）。
pub fn parse(source: &str) -> Result<syn::File, syn::Error> {
    syn::parse_file(source)
}

/// 扫描一份 Rust 源码，收集**标识符**（含路径的各段）。
///
/// # 为什么收集路径的**各段**而不是整条路径
///
/// 断言的形态通常是「有没有出现 `sqlx::`」或「有没有出现 `FromRow`」——
/// 按**段**收集（`sqlx`、`FromRow`）让调用方可以直接用 `contains` 判定，
/// 不必关心它在源码里是 `sqlx::Type` 还是 `use sqlx::Type`。
///
/// 代价是**可能的误报**（`sqlx` 这个单词单独出现也会被收集）——
/// 对「禁止出现某关键词」的检查来说这是**保守**方向，可以接受。
pub fn scan_code(source: &str) -> Result<ScanResult, syn::Error> {
    let file = parse(source)?;
    let mut collector = Collector::default();
    collector.visit_file(&file);
    Ok(collector.result())
}

/// 扫描一份 Rust 源码，收集**标识符**与**字符串字面量**。
///
/// 见模块文档的「语义边界」表。
pub fn scan_code_and_strings(source: &str) -> Result<ScanResult, syn::Error> {
    let file = parse(source)?;
    let mut collector = Collector {
        collect_strings: true,
        ..Collector::default()
    };
    collector.visit_file(&file);
    Ok(collector.result())
}

/// AST 遍历器。
#[derive(Default)]
struct Collector {
    identifiers: BTreeSet<String>,
    paths: BTreeSet<String>,
    calls: BTreeSet<String>,
    strings: BTreeSet<String>,
    /// 是否收集字符串字面量。
    collect_strings: bool,
    /// 收集到的 SQL 写操作。
    sql_writes: BTreeSet<SqlWrite>,
}

impl Collector {
    fn result(self) -> ScanResult {
        ScanResult {
            identifiers: self.identifiers,
            paths: self.paths,
            calls: self.calls,
            strings: self.strings,
            parse_errors: Vec::new(),
            sql_writes: self.sql_writes,
        }
    }

    /// 从 `use` 树里收集**完整路径**。
    ///
    /// ⚠️ `use` 的路径**不是** `syn::Path` —— 它是 `UseTree`。
    /// 不单独处理就会**漏掉所有 `use` 引入的路径** ——
    /// 而那正是「引用了某个 crate 的具体类型」最常见的形式。
    ///
    /// # ⚠️ 记录**所有前缀**
    ///
    /// `use axum::response::sse::Sse;` 记出**两条**：
    ///
    /// | 形态 | 用途 |
    /// |---|---|
    /// | `axum::response::sse` | 判定「用了 `axum::response::sse` 这个**模块**」 |
    /// | `axum::response::sse::Sse` | 判定「用了 `Sse` 这个**类型**」 |
    ///
    /// 只记一条会让其中一种判定**永远为假** ——
    /// 而「永远为假」的断言看起来是绿的，实际什么都没拦。
    fn collect_use_tree(&mut self, tree: &syn::UseTree) {
        let syn::UseTree::Path(use_path) = tree else {
            return;
        };
        let mut segments = Vec::new();
        let mut current = use_path;
        loop {
            segments.push(current.ident.to_string());
            // ⚠️ 每一层都记一次 —— 前缀也是**有意义**的路径。
            self.paths.insert(segments.join("::"));
            match &*current.tree {
                syn::UseTree::Path(next) => current = next,
                syn::UseTree::Name(name) => {
                    segments.push(name.ident.to_string());
                    self.paths.insert(segments.join("::"));
                    break;
                }
                syn::UseTree::Rename(rename) => {
                    segments.push(rename.ident.to_string());
                    self.paths.insert(segments.join("::"));
                    break;
                }
                syn::UseTree::Glob(_) | syn::UseTree::Group(_) => break,
            }
        }
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        // 收集路径的**每一段** —— `sqlx::Type` → {`sqlx`, `Type`}。
        for segment in &path.segments {
            self.identifiers.insert(segment.ident.to_string());
        }
        // ⚠️ 同时收集**完整路径** —— `axum::response::sse`。
        //
        // `syn::Path` 的 `to_token_stream` 会带上**泛型参数**
        // （`Vec<T>` → `Vec < T >`）—— 那会让字符串形态变得**难看**。
        // 所以这里**手工**拼接段名，不带参数。
        let joined: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        self.paths.insert(joined.join("::"));
        syn::visit::visit_path(self, path);
    }

    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        self.identifiers.insert(ident.to_string());
        syn::visit::visit_ident(self, ident);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        // 记录**被调用的**函数名 —— `sleep(...)` → `sleep`。
        //
        // ⚠️ 只记**最后一段**（`tokio::time::sleep(...)` → `sleep`）——
        // 调用方要判定的是「用了哪个**函数**」，
        // 而路径的完整形态由 `paths` 负责。
        if let syn::Expr::Path(path) = &*call.func
            && let Some(last) = path.path.segments.last()
        {
            self.calls.insert(last.ident.to_string());
        }
        syn::visit::visit_expr_call(self, call);
    }

    /// ⚠️ **方法调用** —— `x.sleep()`
    ///
    /// 方法调用的方法名是 `Ident`（**不是** `Path`）——
    /// 它走的是 `visit_expr_method_call`，**不**经过 `visit_expr_call`。
    ///
    /// 不单独处理就会**漏掉所有方法调用** ——
    /// 而 `sleep()` / `interval()` 这类判定恰恰是**方法调用**。
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.calls.insert(call.method.to_string());
        syn::visit::visit_expr_method_call(self, call);
    }

    /// `use` 语句 —— `use axum::response::sse::Sse;`
    ///
    /// ⚠️ `use` 的路径**不是** `syn::Path` —— 它是 `UseTree`。
    /// 不单独处理就会**漏掉所有 `use` 引入的路径** ——
    /// 而那正是「引用了某个 crate 的具体类型」最常见的形式。
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.collect_use_tree(&item.tree);
        syn::visit::visit_item_use(self, item);
    }

    fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
        self.collect_use_tree(tree);
        syn::visit::visit_use_tree(self, tree);
    }

    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
        if self.collect_strings {
            self.strings.insert(lit.value());
        }
        // ⚠️ SQL 写操作的识别**独立**于 `collect_strings` ——
        //
        // 「这个 crate 能不能写这张表」是**安全**判定，
        // 不是「顺便看看」的观察。哪怕调用方只用了 `code_only` 视图，
        // 写操作的判定也必须成立 ——
        // 否则「用 `code_only` 扫描」就成了一条绕过写权限的路径。
        for write in extract_sql_writes(&lit.value()) {
            self.sql_writes.insert(write);
        }
        syn::visit::visit_lit_str(self, lit);
    }

    /// ⚠️ **跳过文档注释**
    ///
    /// `syn` 把 `///` 与 `//!` 解析成 `#[doc = "..."]` **属性** ——
    /// 属性的值是一个 `LitStr`。
    ///
    /// 若不跳过，**文档注释的内容就会混进字符串集合** ——
    /// 那正是「注释里的 `FromRow` / `workflow_runs` 被当成真实引用」的形态。
    ///
    /// ⚠️ 这**必须**处理：`syn` 的 AST 里「注释」表现为
    /// 「`doc` 属性的字符串值」—— 不跳过它就等于**没剥注释**。
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        // `#[doc = "..."]` —— 文档注释。
        //
        // ⚠️ 只按**属性名**判定，不看值 —— 因为 `#[doc = include_str!("...")]`
        // 之类的写法也存在，而它们**同样**是文档。
        if attr.path().is_ident("doc") {
            return;
        }
        syn::visit::visit_attribute(self, attr);
    }
}

/// 从一段 SQL 文本里提取**写操作**及其目标表。
///
/// # 判据
///
/// 只认三种操作：`INSERT` / `UPDATE` / `DELETE`。
/// 读操作（`SELECT`）**不算** —— 架构文档 §4.2 差异 1 判定的
/// 是**写入权**，读不构成写入。
///
/// # 为什么用**逐词**扫描而不是「找 `INSERT INTO` 子串」
///
/// SQL 的写法很多：
///
/// | 写法 | 子串匹配能识别吗 |
/// |---|---|
/// | `INSERT INTO t (...)` | ✅ |
/// | `insert into t (...)` | ✅（若大小写不敏感） |
/// | `INSERT  INTO t`（多空格） | ❌ |
/// | `INSERT\nINTO t`（换行） | ❌ |
/// | `UPDATE t SET ...` | ✅ |
/// | `WITH x AS (...) UPDATE t SET ...` | ❌ |
///
/// 而**表名**的提取更麻烦：`INSERT INTO schema.table`、
/// `UPDATE "workflow_runs" SET ...`（带引号）都存在。
///
/// 逐词扫描把它们统一成「词序列」：
///
/// ```text
/// INSERT INTO workflow_runs   →  ["insert", "into", "workflow_runs"]
/// UPDATE "workflow_runs" SET  →  ["update", "workflow_runs", "set"]
/// ```
///
/// 然后按**词的位置**判定操作与目标表。
///
/// # 为什么**不能**把「含表名的字符串」都算成写
///
/// 那会把 `SELECT * FROM workflow_runs` 也算成写 ——
/// 而 §4.2 差异 1 判的是**写入权**。读写不分，判定就失去意义。
///
/// # 建表 DDL 为什么**不算**写操作
///
/// `CREATE TABLE workflow_runs (...)` 是**建表**，不是**访问**。
/// 架构文档 §4.3.2 约束 2 明确允许 `nexus-infra` 做建表 DDL。
/// `CREATE` 不在写操作的三种里 —— 这是**刻意的**。
///
/// # 词法规则
///
/// - 标识符：`[A-Za-z_][A-Za-z0-9_$]*`
/// - 引号标识符：`"..."` —— 去掉引号后按普通标识符处理
/// - 字符串字面量：`'...'` —— **整体跳过**（它是值，不是表名）
/// - 其它字符一律是分隔符
pub fn extract_sql_writes(sql: &str) -> Vec<SqlWrite> {
    let words = sql_words(sql);
    let mut writes = Vec::new();

    let mut index = 0;
    while index < words.len() {
        // ⚠️ 关键字匹配**大小写不敏感** ——
        // SQL 的关键字大小写不敏感（`insert into` 与 `INSERT INTO` 等价），
        // 只认大写会在小写写法上**静默漏报**。
        let keyword = words[index].to_ascii_lowercase();
        match keyword.as_str() {
            "insert" => {
                // `INSERT INTO <table>` —— `INTO` 可省略（`INSERT <table>`）。
                let mut cursor = index + 1;
                if words
                    .get(cursor)
                    .is_some_and(|w| w.eq_ignore_ascii_case("into"))
                {
                    cursor += 1;
                }
                if let Some(table) = words.get(cursor) {
                    writes.push(SqlWrite {
                        operation: "INSERT".to_string(),
                        table: table.clone(),
                    });
                }
                index = cursor;
            }
            "update" => {
                if let Some(table) = words.get(index + 1) {
                    writes.push(SqlWrite {
                        operation: "UPDATE".to_string(),
                        table: table.clone(),
                    });
                }
                index += 1;
            }
            "delete" => {
                // `DELETE FROM <table>` —— `FROM` 可省略（`DELETE <table>`）。
                let mut cursor = index + 1;
                if words
                    .get(cursor)
                    .is_some_and(|w| w.eq_ignore_ascii_case("from"))
                {
                    cursor += 1;
                }
                if let Some(table) = words.get(cursor) {
                    writes.push(SqlWrite {
                        operation: "DELETE".to_string(),
                        table: table.clone(),
                    });
                }
                index = cursor;
            }
            _ => index += 1,
        }
    }

    writes
}

/// 把一段 SQL 拆成**词序列**。
///
/// # 为什么跳过字符串字面量
///
/// `'...'` 里的一切都是**值**，不是标识符 ——
/// `WHERE name = 'update workflow_runs'` 里的 `update` 不是写操作。
fn sql_words(sql: &str) -> Vec<String> {
    let mut words = Vec::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            // 单引号字符串 —— **整体跳过**。
            '\'' => {
                index += 1;
                while index < chars.len() {
                    if chars[index] == '\'' {
                        // `''` 是转义的单引号 —— 不算结束。
                        if chars.get(index + 1) == Some(&'\'') {
                            index += 2;
                            continue;
                        }
                        break;
                    }
                    index += 1;
                }
                index += 1;
            }
            // 双引号标识符 —— 去掉引号，按普通标识符处理。
            '"' => {
                index += 1;
                let mut name = String::new();
                while index < chars.len() && chars[index] != '"' {
                    name.push(chars[index]);
                    index += 1;
                }
                index += 1;
                if !name.is_empty() {
                    words.push(name);
                }
            }
            // 标识符字符。
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while index < chars.len() {
                    let c = chars[index];
                    if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                        name.push(c);
                        index += 1;
                    } else {
                        break;
                    }
                }
                words.push(name);
            }
            _ => index += 1,
        }
    }

    words
}

/// 递归收集目录下的所有 `.rs` 文件。
pub fn walk_rs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs(dir, &mut out);
    out.sort();
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// 扫描一个目录下的所有 `.rs` 文件。
///
/// # ⚠️ 解析失败**不会**被忽略
///
/// 解析失败的文件会进 [`ScanResult::parse_errors`]，
/// 调用方**应当**用 [`ScanResult::has_parse_errors`] 检查并让门禁变红。
///
/// # 参数
///
/// `collect_strings` —— 是否收集字符串字面量（见模块文档的「语义边界」）。
pub fn scan_dir(dir: &Path, collect_strings: bool) -> ScanResult {
    let mut merged = ScanResult::default();
    for file in walk_rs(dir) {
        let Ok(source) = std::fs::read_to_string(&file) else {
            merged.parse_errors.push((file, "读取失败".to_string()));
            continue;
        };
        let scanned = if collect_strings {
            scan_code_and_strings(&source)
        } else {
            scan_code(&source)
        };
        match scanned {
            Ok(result) => {
                merged.identifiers.extend(result.identifiers);
                merged.paths.extend(result.paths);
                merged.calls.extend(result.calls);
                merged.strings.extend(result.strings);
                merged.sql_writes.extend(result.sql_writes);
            }
            Err(err) => merged.parse_errors.push((file, err.to_string())),
        }
    }
    merged
}

/// 一个**被推进**（自增 / 赋值）的变量。
///
/// # 为什么用 AST 而不是「逐行搜文本」
///
/// 旧实现用「逐行找 `seq += ` / `seq = `」来判定「序号在**唯一**的地方推进」。
/// 那有**两个**问题：
///
/// 1. **假阳性** —— 一行注释里写 `// seq += 1` 会被算成一处推进；
/// 2. **假阴性** —— 写成 `self.seq += 1` 或 `*counter += 1` 会被漏掉。
///
/// AST 层面判定**只看真正的赋值表达式**，两个问题同时消失。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advancement {
    /// 被推进的变量名（如 `seq`）。
    pub name: String,
    /// 推进的**形态** —— `+=` / `-=` / `=` / `fetch_add` / `fetch_sub`。
    pub kind: String,
}

/// 找出源码里所有「**推进某个变量**」的位置。
///
/// # 支持的形态
///
/// | 形态 | AST 形状 |
/// |---|---|
/// | `x += 1` | [`syn::Expr::Binary`] + [`syn::BinOp::AddAssign`] |
/// | `x -= 1` | [`syn::Expr::Binary`] + [`syn::BinOp::SubAssign`] |
/// | `x = ...` | [`syn::Expr::Assign`] |
/// | `x.fetch_add(..)` | [`syn::Expr::MethodCall`] |
/// | `x.fetch_sub(..)` | [`syn::Expr::MethodCall`] |
///
/// # ⚠️ 匹配的是**最后一段**名字
///
/// `self.seq += 1` 记为 `seq`；`*counter += 1` **不**匹配
/// （解引用不是路径 —— 它是别的形态，判定要更精确时应另行处理）。
pub fn find_advancements(source: &str, target: &str) -> Result<Vec<Advancement>, syn::Error> {
    let file = parse(source)?;
    let mut collector = AdvancementCollector {
        target: target.to_string(),
        found: Vec::new(),
    };
    collector.visit_file(&file);
    Ok(collector.found)
}

/// 推进检测的 AST 遍历器。
struct AdvancementCollector {
    /// 目标变量名。
    target: String,
    /// 找到的推进点。
    found: Vec<Advancement>,
}

impl AdvancementCollector {
    /// 若「推进某个变量」，记录它。
    fn record(&mut self, kind: &str, name: Option<String>) {
        if let Some(name) = name
            && name == self.target
        {
            self.found.push(Advancement {
                name,
                kind: kind.to_string(),
            });
        }
    }
}

impl<'ast> Visit<'ast> for AdvancementCollector {
    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        match expr {
            // `x = ...`
            syn::Expr::Assign(assign) => {
                if let syn::Expr::Path(path) = &*assign.left {
                    self.record("=", path.path.segments.last().map(|s| s.ident.to_string()));
                }
            }
            // `x += 1` / `x -= 1`
            syn::Expr::Binary(binary) => {
                let kind = match binary.op {
                    syn::BinOp::AddAssign(_) => Some("+="),
                    syn::BinOp::SubAssign(_) => Some("-="),
                    _ => None,
                };
                if let Some(kind) = kind
                    && let syn::Expr::Path(path) = &*binary.left
                {
                    self.record(kind, path.path.segments.last().map(|s| s.ident.to_string()));
                }
            }
            // `x.fetch_add(..)` / `x.fetch_sub(..)`
            syn::Expr::MethodCall(call) => {
                let method = call.method.to_string();
                if method == "fetch_add" || method == "fetch_sub" {
                    let name = match &*call.receiver {
                        syn::Expr::Path(path) => {
                            path.path.segments.last().map(|s| s.ident.to_string())
                        }
                        _ => None,
                    };
                    self.record(&method, name);
                }
            }
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }
}
