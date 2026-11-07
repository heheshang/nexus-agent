//! `nexus-workflow` 的**结构层**断言。
//!
//! # 为什么这些断言不可省略
//!
//! 行为测试只能证明「**当前实现**产出了正确的结果」——
//! 它拦不住「有人加了一条 `sqlx::query!`，结果仍然对，但依赖方向已经坏了」。
//!
//! 本 crate 的**边界**是**已定稿**条款（架构文档 §6.3 第 5 项）：
//!
//! - `nexus-workflow` 是**能力层** —— **不得**依赖 `nexus-repo`（数据层）；
//! - `nexus-workflow` **不负责持久化** —— 源码里**不该**出现 `sqlx`。
//!
//! # 检验的设计条款
//!
//! | 测试 | 检验的条款 |
//! |---|---|
//! | `the_workflow_crate_does_not_depend_on_the_data_layer` | §4.1.2 能力层不得反向依赖数据层 |
//! | `the_workflow_crate_contains_no_database_access` | §4.4 规则 3（`sqlx` 只允许出现在三个 crate） |
//! | `the_workflow_crate_documents_its_responsibility_boundary` | AC #11（crate 级文档写明职责边界） |
//! | `the_workflow_crate_does_not_use_box_dyn_on_the_node_hot_path` | AC #12（节点热路径不含 `Box<dyn>`） |
//! | `the_dyn_trait_object_only_appears_at_the_graph_loading_site` | AC #12（`Arc<dyn NodeInvoke>` 只在装载处） |
//!
//! # 为什么用 **token 级扫描**而不是字符串查找
//!
//! 字符串查找会把**文档注释**里的 `sqlx` 一词也算进去 ——
//! 而 crate 级文档**恰恰要**说明「本 crate 不需要 `sqlx`」。
//!
//! 那样断言就会**恒红**（或被迫写成「注释里不算」的特殊逻辑）。
//! token 级扫描只认**标识符**与**路径** —— 注释里的词**不是**标识符。
//!
//! 这正是 NEX-48 建立的纪律：**源码扫描类断言必须剥离注释**。

use std::fs;
use std::path::{Path, PathBuf};

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
//
// ⚠️ 为什么用 `#[path]` 而不是抽一个 crate
//
// 抽 crate 会**新增**一条依赖边，而本仓库的依赖方向是**已定稿**条款
// （架构文档 §6.3 第 5 项）。`#[path]` include 只需要**文件系统**上的相对路径，
// **不产生任何依赖边** —— `Cargo.toml` 完全不动。
#[path = "../../nexus-core/tests/support/token_scan.rs"]
mod token_scan;

use syn::visit::Visit;
use syn::{GenericArgument, Type};

/// `nexus-workflow` 的 `src` 目录。
fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 遍历 `src` 下的所有 `.rs` 文件。
fn source_files() -> Vec<PathBuf> {
    token_scan::walk_rs(&src_dir())
}

/// 读一个源文件的内容。
fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("读取源文件失败")
}

/// **§4.1.2** —— 能力层**不得**反向依赖数据层。
///
/// `nexus-workflow` 是图引擎（**能力层**），`nexus-repo` 是数据层。
/// 能力层依赖数据层就是**反向依赖** —— 它会让「纯内存的图编译」
/// 变成「必须有数据库才能编译图」。
///
/// ⚠️ 断言的是 **Cargo.toml 的依赖声明** ——
/// 那才是依赖方向的**唯一**来源（源码里的 `use` 只是它的结果）。
#[test]
fn the_workflow_crate_does_not_depend_on_the_data_layer() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let content = read(&manifest);

    for forbidden in ["nexus-repo", "nexus-execution", "nexus-infra"] {
        assert!(
            !content.contains(forbidden),
            "nexus-workflow 不得依赖 {forbidden} —— 能力层不得反向依赖数据层（§4.1.2）"
        );
    }
}

/// **§4.4 规则 3** —— `sqlx` 只允许出现在 `nexus-repo` / `nexus-infra` / `nexus-execution`。
///
/// `nexus-workflow` **不负责持久化** —— 源码里**不该**出现任何 `sqlx` 痕迹。
///
/// ⚠️ 用 **token 级扫描**（AST 级）—— 注释里的 `sqlx` 一词**不算**。
#[test]
fn the_workflow_crate_contains_no_database_access() {
    for path in source_files() {
        let source = read(&path);
        let scan = token_scan::scan_code(&source)
            .unwrap_or_else(|e| panic!("{} 解析失败：{e}", path.display()));

        assert!(
            !scan.has_parse_errors(),
            "{} 解析失败 —— 扫描结果不完整，断言会静默失效",
            path.display()
        );
        assert!(
            !scan.mentions("sqlx"),
            "{} 出现了 sqlx —— nexus-workflow 不负责持久化（§4.4 规则 3）",
            path.display()
        );
        assert!(
            !scan.mentions("FromRow"),
            "{} 出现了 FromRow —— 行类型属于 nexus-repo",
            path.display()
        );
    }
}

/// **AC #11** —— crate 级文档写明「本 crate 负责图引擎 / 不负责持久化」。
///
/// ⚠️ 这条断言看似「查字符串」，但它守的是一条**契约**：
/// 职责边界必须**写下来** —— 只在代码里成立、文档里不写的边界，
/// 下一个人会以为「顺手加个持久化也没事」。
#[test]
fn the_workflow_crate_documents_its_responsibility_boundary() {
    let lib = src_dir().join("lib.rs");
    let content = read(&lib);

    assert!(
        content.contains("图引擎"),
        "crate 级文档必须写明本 crate 负责图引擎"
    );
    assert!(
        content.contains("持久化"),
        "crate 级文档必须写明本 crate 不负责持久化"
    );
}

// ────────────────────────────────────────────────────────────────────
// AC #12 —— 节点执行热路径不含 `Box<dyn>`
// ────────────────────────────────────────────────────────────────────

/// 收集源码里的**所有** trait 对象（`dyn Trait`）及其**宿主类型**。
///
/// # 返回
///
/// `(宿主类型名, trait 名)` 的列表 —— 宿主是 `Box` / `Arc` / 其它。
///
/// # ⚠️ 为什么必须**解析类型**而不是**搜字符串**
///
/// `Box<dyn` 这个**文本**在源码里可能出现在：
///
/// - 注释里（本 crate 的文档就提到它）；
/// - 字符串字面量里；
/// - **真正的类型标注**里。
///
/// 只有第三种是**违规**。用字符串查找会**恒红**，
/// 逼着人加「注释里不算」的特殊逻辑 —— 那是**绕过**门禁的起点。
///
/// `syn` 的 AST 里，trait 对象是 [`syn::Type::TraitObject`] 节点 ——
/// 「注释里不算」**自动成立**。
fn collect_trait_objects(source: &str) -> Result<Vec<(String, String)>, syn::Error> {
    let file = token_scan::parse(source)?;
    let mut finder = TraitObjectFinder::default();
    finder.visit_file(&file);
    Ok(finder.found)
}

/// trait 对象收集器。
#[derive(Default)]
struct TraitObjectFinder {
    /// 找到的 `(宿主类型名, trait 名)`。
    found: Vec<(String, String)>,
}

impl Visit<'_> for TraitObjectFinder {
    /// 访问一个**类型**。
    fn visit_type(&mut self, ty: &Type) {
        if let Type::Path(path) = ty {
            // 路径的**最后一段**是宿主类型（`Box` / `Arc` / …）。
            let Some(last) = path.path.segments.last() else {
                syn::visit::visit_type(self, ty);
                return;
            };
            let host = last.ident.to_string();

            // 泛型实参里找 `dyn Trait`。
            if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                for arg in &args.args {
                    if let GenericArgument::Type(inner) = arg
                        && let Type::TraitObject(obj) = inner
                    {
                        for bound in &obj.bounds {
                            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                                self.found.push((
                                    host.clone(),
                                    trait_bound
                                        .path
                                        .segments
                                        .last()
                                        .map(|s| s.ident.to_string())
                                        .unwrap_or_default(),
                                ));
                            }
                        }
                    }
                }
            }
        }
        syn::visit::visit_type(self, ty);
    }
}

/// **AC #12** —— 节点执行热路径**不含** `Box<dyn>`。
///
/// # 为什么是**全 crate** 禁止，而不是「热路径」判定
///
/// 「热路径」在源码里**没有**可判定的标记 ——
///
/// - 有人说「`NodeRegistry` 不算热路径」；
/// - 有人说「这个 `Box<dyn>` 在辅助函数里，不在节点主循环里」。
///
/// 那样的争论**无法机械判定** —— 于是这条断言就**长期失效**。
///
/// 所以本断言取**更强**的形式：**整个 crate 的 `src/` 里不出现 `Box<dyn>`**。
///
/// # ⚠️ 这比 AC 原文**更严**，但**不冲突**
///
/// AC #12 说「不含 `Box<dyn>` 于节点执行热路径」。
/// 全 crate 禁止 = 热路径必然不含。**更严**的形式满足**更松**的要求。
///
/// # 为什么「更严」是对的
///
/// `Box<dyn>` 在本 crate 里**没有任何正当用途**：
/// 异构集合用 `Arc<dyn>`（共享所有权），热路径用泛型（静态分发）。
/// `Box<dyn>` 只会在「有人图省事把 trait 对象装箱」时出现 ——
/// 那正是要拦的。
#[test]
fn the_workflow_crate_does_not_use_box_dyn_on_the_node_hot_path() {
    for path in source_files() {
        let source = read(&path);
        let objects = collect_trait_objects(&source)
            .unwrap_or_else(|e| panic!("{} 解析失败：{e}", path.display()));

        let boxed: Vec<_> = objects.iter().filter(|(host, _)| host == "Box").collect();

        assert!(
            boxed.is_empty(),
            "{} 出现了 Box<dyn> —— 节点执行是逐节点的热路径，必须走泛型 + 静态分发；\
             异构集合用 Arc<dyn>（{}）",
            path.display(),
            boxed
                .iter()
                .map(|(_, trait_name)| trait_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

/// **AC #12** —— `Arc<dyn NodeInvoke>` **只**出现在**图定义装载**处。
///
/// # 判据
///
/// `Arc<dyn ...>` 只允许出现在 `src/node.rs` 的
/// [`nexus_workflow::node::NodeRegistry`] 里 ——
/// 那是「异构集合」的唯一合法场景（架构文档 §2.3.3）。
///
/// # ⚠️ 为什么**只**允许 `NodeInvoke`
///
/// 别的 trait 的 `Arc<dyn>` 说明有人在**别处**引入了动态分发 ——
/// 而那些地方**没有**「异构集合」的理由。
#[test]
fn the_dyn_trait_object_only_appears_at_the_graph_loading_site() {
    let node_rs = src_dir().join("node.rs");

    for path in source_files() {
        let source = read(&path);
        let objects = collect_trait_objects(&source)
            .unwrap_or_else(|e| panic!("{} 解析失败：{e}", path.display()));

        let arcs: Vec<_> = objects.iter().filter(|(host, _)| host == "Arc").collect();

        for (_, trait_name) in &arcs {
            assert_eq!(
                *trait_name,
                "NodeInvoke",
                "{} 出现了 Arc<dyn {trait_name}> —— 只有 NodeInvoke 是异构集合，\
                 需要 dyn（架构文档 §2.3.3）",
                path.display()
            );
            assert_eq!(
                path,
                node_rs,
                "{} 出现了 Arc<dyn {trait_name}> —— Arc<dyn NodeInvoke> 只允许出现在\
                 图定义装载处（src/node.rs 的 NodeRegistry），热路径必须走泛型",
                path.display()
            );
        }
    }
}

/// **AC #12** —— 节点注册表**确实**用 `Arc<dyn NodeInvoke>` 装载。
///
/// # ⚠️ 为什么这条**必要**
///
/// 上一条断言是「`Arc<dyn>` 只能出现在 `node.rs`」——
/// 如果哪天有人把 `NodeRegistry` 改成泛型，那条断言会**恒真**（不再有 `Arc<dyn>`），
/// 于是**看起来**通过了，实际是「规则失效」而不是「规则被满足」。
///
/// 所以必须**同时**断言「该出现的地方**确实**出现了」——
/// 两条合起来才是完整的规则。
#[test]
fn the_graph_loading_site_uses_arc_dyn_node_invoke() {
    let node_rs = src_dir().join("node.rs");
    let source = read(&node_rs);
    let objects = collect_trait_objects(&source)
        .unwrap_or_else(|e| panic!("{} 解析失败：{e}", node_rs.display()));

    assert!(
        objects
            .iter()
            .any(|(host, trait_name)| host == "Arc" && trait_name == "NodeInvoke"),
        "src/node.rs 必须用 Arc<dyn NodeInvoke> 装载异构节点集合 —— \
         没有它，「Arc<dyn 只在装载处」这条规则就是空的"
    );
}
