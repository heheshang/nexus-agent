//! 图编译的行为层测试。
//!
//! 覆盖 NEX-38 的验收条件：
//!
//! | AC | 测试 |
//! |---|---|
//! | #4 | `compiled_graph_exposes_nodes_roots_and_name` |
//! | #5 | `all_six_node_kinds_survive_compilation` |
//! | #6 | `node_waits_for_all_predecessors` |
//! | #7 | `cycle_is_rejected_instead_of_hanging` |
//! | #8 | `recursive_subgraph_is_rejected_instead_of_expanding_forever` |
//! | #9 | `each_expansion_limit_triggers_independently` |
//! | #10 | `limit_violations_map_to_budget_exceeded_not_panic` |

use std::collections::{BTreeMap, BTreeSet};

use nexus_core::error::ErrorCode;
use nexus_workflow::graph::{
    CompileError, GraphDefinition, MAX_STATE_BYTES, NodeId, NodeKind, NodeSpec, check_state_size,
    compile,
};

/// 构造一个 lambda 节点。
fn lambda(id: &str, next: &[&str]) -> NodeSpec {
    NodeSpec {
        id: NodeId::new(id),
        kind: NodeKind::Lambda {
            invoke: format!("invoke_{id}"),
        },
        next: next.iter().map(|n| NodeId::new(*n)).collect(),
    }
}

/// 构造一个分支节点。
fn branch(id: &str, yes: &str, no: &str) -> NodeSpec {
    NodeSpec {
        id: NodeId::new(id),
        kind: NodeKind::Branch {
            condition_key: "flag".to_string(),
            yes: NodeId::new(yes),
            no: NodeId::new(no),
        },
        next: Vec::new(),
    }
}

/// 构造一个**子图引用**节点。
fn subgraph(id: &str, name: &str, next: &[&str]) -> NodeSpec {
    NodeSpec {
        id: NodeId::new(id),
        kind: NodeKind::SubGraph {
            name: NodeId::new(name),
        },
        next: next.iter().map(|n| NodeId::new(*n)).collect(),
    }
}

/// 构造一个图。
fn graph(name: &str, nodes: Vec<NodeSpec>) -> GraphDefinition {
    GraphDefinition {
        name: name.to_string(),
        nodes,
        subgraphs: BTreeMap::new(),
    }
}

/// 构造一个图，**带**子图注册表。
fn graph_with_subgraphs(
    name: &str,
    nodes: Vec<NodeSpec>,
    subgraphs: &[(&str, GraphDefinition)],
) -> GraphDefinition {
    GraphDefinition {
        name: name.to_string(),
        nodes,
        subgraphs: subgraphs
            .iter()
            .map(|(key, value)| (NodeId::new(*key), value.clone()))
            .collect(),
    }
}

/// 把节点键列表转成集合。
fn set(ids: &[&str]) -> BTreeSet<NodeId> {
    ids.iter().map(|id| NodeId::new(*id)).collect()
}

/// **AC #4** —— 编译产物含 `nodes` / `roots` / `name`。
#[test]
fn compiled_graph_exposes_nodes_roots_and_name() {
    let definition = graph(
        "demo",
        vec![lambda("a", &["b"]), lambda("b", &["c"]), lambda("c", &[])],
    );

    let compiled = compile(&definition).expect("合法的图应编译成功");

    assert_eq!(compiled.name, "demo");
    assert_eq!(compiled.nodes.len(), 3);
    assert_eq!(compiled.roots, vec![NodeId::new("a")]);
}

/// **AC #7** —— 拓扑排序：前驱总在后继之前。
#[test]
fn compilation_orders_predecessors_before_successors() {
    let definition = graph(
        "ordered",
        vec![lambda("c", &[]), lambda("a", &["c"]), lambda("b", &["c"])],
    );

    let compiled = compile(&definition).expect("合法的图应编译成功");

    let positions: Vec<usize> = ["a", "b", "c"]
        .iter()
        .map(|id| {
            compiled
                .nodes
                .iter()
                .position(|n| n.id.as_str() == *id)
                .expect("节点应存在")
        })
        .collect();

    assert!(
        positions[2] > positions[0] && positions[2] > positions[1],
        "前驱必须排在后继之前，实际顺序：{positions:?}"
    );
}

/// **AC #5** —— 六种节点种类**全部**存在，且编译后保持不变。
#[test]
fn all_six_node_kinds_survive_compilation() {
    let sub = graph("sub", vec![lambda("s1", &["s2"]), lambda("s2", &[])]);

    let definition = graph_with_subgraphs(
        "kinds",
        vec![
            lambda("l", &["b"]),
            branch("b", "h", "t"),
            subgraph("sg", "sub", &[]),
            NodeSpec {
                id: NodeId::new("p"),
                kind: NodeKind::Parallel {
                    branches: vec![NodeId::new("l"), NodeId::new("h")],
                },
                next: Vec::new(),
            },
            NodeSpec {
                id: NodeId::new("h"),
                kind: NodeKind::Human {
                    prompt: "确认？".to_string(),
                },
                next: Vec::new(),
            },
            NodeSpec {
                id: NodeId::new("t"),
                kind: NodeKind::Tool {
                    tool_code: "db_query".to_string(),
                },
                next: Vec::new(),
            },
        ],
        &[("sub", sub)],
    );

    let compiled = compile(&definition).expect("合法的图应编译成功");

    let kinds: Vec<&NodeKind> = compiled.nodes.iter().map(|n| &n.kind).collect();

    assert!(
        kinds.iter().any(|k| matches!(k, NodeKind::Lambda { .. })),
        "Lambda 应存在"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, NodeKind::Branch { .. })),
        "Branch 应存在"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, NodeKind::SubGraph { .. })),
        "SubGraph 应存在"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, NodeKind::Parallel { .. })),
        "Parallel 应存在"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, NodeKind::Human { .. })),
        "Human 应存在"
    );
    assert!(
        kinds.iter().any(|k| matches!(k, NodeKind::Tool { .. })),
        "Tool 应存在"
    );
}

/// **AC #6** —— 触发语义是 `AllPredecessor`：
/// **所有**前驱完成后才触发。
#[test]
fn node_waits_for_all_predecessors() {
    // a 和 b 都指向 c。
    let definition = graph(
        "join",
        vec![lambda("a", &["c"]), lambda("b", &["c"]), lambda("c", &[])],
    );

    let compiled = compile(&definition).expect("合法的图应编译成功");
    let c = NodeId::new("c");

    // 两个前驱**只完成一个** → 不可触发。
    assert!(
        !compiled.is_ready(&c, &set(&["a"])),
        "只完成一个前驱时不得触发"
    );
    assert!(
        !compiled.is_ready(&c, &set(&["b"])),
        "只完成另一个前驱时也不得触发"
    );
    // 一个都没完成 → 不可触发。
    assert!(!compiled.is_ready(&c, &set(&[])), "无前驱完成时不得触发");
    // 两个都完成 → 可触发。
    assert!(
        compiled.is_ready(&c, &set(&["a", "b"])),
        "所有前驱完成时应可触发"
    );
}

/// **AC #6（续）** —— 只有一个前驱的节点，完成该前驱即可触发。
#[test]
fn node_with_single_predecessor_triggers_after_it_completes() {
    let definition = graph("single", vec![lambda("a", &["b"]), lambda("b", &[])]);
    let compiled = compile(&definition).expect("合法的图应编译成功");
    let b = NodeId::new("b");

    assert!(!compiled.is_ready(&b, &set(&[])));
    assert!(compiled.is_ready(&b, &set(&["a"])));
}

/// **AC #7** —— 图有**环**时编译**失败**（不是死循环）。
#[test]
fn cycle_is_rejected_instead_of_hanging() {
    // a → b → c → a
    let definition = graph(
        "cyclic",
        vec![
            lambda("a", &["b"]),
            lambda("b", &["c"]),
            lambda("c", &["a"]),
        ],
    );

    let err = compile(&definition).expect_err("有环的图必须编译失败");

    match err {
        CompileError::Cycle { cycle } => {
            assert!(!cycle.is_empty(), "环信息不应为空");
            // 环上的节点必须**首尾相接** —— 报错要能指出哪条链构成了环。
            assert_eq!(cycle.first(), cycle.last(), "环应当首尾相接");
        }
        other => panic!("应报 Cycle，实际：{other:?}"),
    }
}

/// **AC #7（续）** —— 自环也要被抓住。
#[test]
fn self_loop_is_rejected() {
    let definition = graph("selfloop", vec![lambda("a", &["a"])]);

    let err = compile(&definition).expect_err("自环必须编译失败");

    assert!(
        matches!(err, CompileError::Cycle { .. }),
        "应报 Cycle，实际：{err:?}"
    );
}

/// **AC #8** —— 子图**递归**检测：编译时用 `HashSet<NodeId>` 检测自引用。
///
/// ⚠️ 子图是**按名注册**的（参考架构 §3.5.1：`definition.Subflows`）——
/// 所以「A 的子图里引用了 A」是**可能**的，
/// 展开时会无限递归 → **栈溢出**。
///
/// 这个用例**必须**被抓住 —— 它是本模块存在的核心理由。
#[test]
fn recursive_subgraph_is_rejected_instead_of_expanding_forever() {
    // 子图 `loop` 里的节点又引用了子图 `loop` 自己。
    let recursive = graph("loop_inner", vec![subgraph("again", "loop", &[])]);
    let definition = graph_with_subgraphs(
        "recursive",
        vec![subgraph("start", "loop", &[])],
        &[("loop", recursive)],
    );

    let err = compile(&definition).expect_err("递归子图必须编译失败");

    match err {
        CompileError::RecursiveSubGraph { path } => {
            assert!(!path.is_empty(), "递归路径不应为空");
            assert_eq!(
                path.first(),
                path.last(),
                "递归路径应当首尾相接 —— 报出「loop → loop」"
            );
            assert_eq!(path.first().expect("路径非空").as_str(), "loop");
        }
        other => panic!("应报 RecursiveSubGraph，实际：{other:?}"),
    }
}

/// **AC #8（续）** —— **互相**递归（A 含 B，B 含 A）也要被抓住。
#[test]
fn mutually_recursive_subgraphs_are_rejected() {
    // b 的子图引用 a；a 的子图引用 b。
    let b_inner = graph("b_inner", vec![subgraph("to_a", "a", &[])]);
    let a_inner = graph("a_inner", vec![subgraph("to_b", "b", &[])]);
    let definition = graph_with_subgraphs(
        "mutual",
        vec![subgraph("start", "a", &[])],
        &[("a", a_inner), ("b", b_inner)],
    );

    let err = compile(&definition).expect_err("互相递归必须编译失败");

    assert!(
        matches!(err, CompileError::RecursiveSubGraph { .. }),
        "应报 RecursiveSubGraph，实际：{err:?}"
    );
}

/// **AC #8（续）** —— **不**递归的同名子图引用要能正常编译。
///
/// ⚠️ 这是「递归检测不是「键重复检测」」的证明 ——
/// 子图 `shared` 被两个不同的地方引用，**不是**递归。
#[test]
fn the_same_subgraph_used_twice_is_not_recursion() {
    let shared = graph(
        "shared_inner",
        vec![lambda("s1", &["s2"]), lambda("s2", &[])],
    );
    let definition = graph_with_subgraphs(
        "twice",
        vec![
            subgraph("first", "shared", &[]),
            subgraph("second", "shared", &[]),
        ],
        &[("shared", shared)],
    );

    let compiled = compile(&definition).expect("同一个子图引用两次不应是递归");

    // 两个容器 + 每个容器各展开一份子图（2 节点）= 2 + 2×2 = 6。
    //
    // ⚠️ 子图**按引用位置**各展开一份（路径前缀是**容器**的键）——
    // 这样两个引用点各自独立，不会共享状态。
    assert_eq!(compiled.nodes.len(), 6);
}

/// **AC #9** —— 展开上限**三个各自生效**。
///
/// 这里逐个验证三个上限是**独立**的 —— 触发其中一个时，
/// 另外两个**没有**越界。
#[test]
fn each_expansion_limit_triggers_independently() {
    // ── 上限一：嵌套深度 > 8 ──
    //
    // 构造 9 层嵌套的子图（每层引用一个不同的子图名）。
    let mut subgraphs: Vec<(String, GraphDefinition)> = Vec::new();
    for level in 1..=9 {
        let child = if level == 9 {
            graph("leaf", vec![lambda("leaf_node", &[])])
        } else {
            graph(
                &format!("inner{level}"),
                vec![subgraph("n", &format!("s{}", level + 1), &[])],
            )
        };
        subgraphs.push((format!("s{level}"), child));
    }
    let refs: Vec<(&str, GraphDefinition)> = subgraphs
        .iter()
        .map(|(name, def)| (name.as_str(), def.clone()))
        .collect();
    let definition = graph_with_subgraphs("deep", vec![subgraph("top", "s1", &[])], &refs);

    let err = compile(&definition).expect_err("深度超限必须编译失败");
    match err {
        CompileError::DepthExceeded { limit, .. } => {
            // ⚠️ 字面量 8 —— 引用常量会让「改了常量」这个注入测不出来。
            assert_eq!(limit, 8, "深度上限应照抄参考架构的 8");
        }
        other => panic!("应报 DepthExceeded，实际：{other:?}"),
    }

    // ── 上限二：展开节点总数 > 128 ──
    //
    // 造 129 个平铺节点 —— 深度是 1（不超限），但节点总数超限。
    //
    // ⚠️ 这里用**字面量 128** 而不是 `MAX_EXPANDED_NODES` ——
    // 用常量会让「改了常量」这个注入**测不出来**
    // （断言跟着常量一起变，永远成立）。
    let wide: Vec<NodeSpec> = (0..=128).map(|i| lambda(&format!("w{i}"), &[])).collect();
    let err = compile(&graph("wide", wide)).expect_err("节点数超限必须编译失败");
    match err {
        CompileError::NodeCountExceeded { limit, .. } => {
            assert_eq!(limit, 128, "节点数上限应照抄参考架构的 128");
        }
        other => panic!("应报 NodeCountExceeded，实际：{other:?}"),
    }

    // ── 上限三：状态大小 > 4 MiB ──
    //
    // ⚠️ 状态大小是**运行期**的上限（图的**状态**）——
    // 编译期还没有状态，所以它不是编译期检查，
    // 而是**独立**的判据函数 `check_state_size`。
    // ⚠️ 字面量 4 MiB —— 引用常量会让「改了常量」这个注入测不出来。
    assert_eq!(
        MAX_STATE_BYTES,
        4 * 1024 * 1024,
        "状态大小上限应照抄参考架构的 4 MiB"
    );
    assert!(
        check_state_size(4 * 1024 * 1024).is_ok(),
        "恰好等于上限不应报错"
    );
    let err = check_state_size(4 * 1024 * 1024 + 1).expect_err("超限必须报错");
    match err {
        CompileError::StateSizeExceeded { size, limit } => {
            assert_eq!(size, 4 * 1024 * 1024 + 1);
            assert_eq!(limit, 4 * 1024 * 1024);
        }
        other => panic!("应报 StateSizeExceeded，实际：{other:?}"),
    }
}

/// **AC #9（续）** —— 恰好 128 个节点**不**超限。
#[test]
fn exactly_the_node_limit_is_accepted() {
    // ⚠️ 字面量 128 —— 引用常量会让「改了常量」这个注入测不出来。
    let wide: Vec<NodeSpec> = (0..128).map(|i| lambda(&format!("w{i}"), &[])).collect();

    let compiled = compile(&graph("exact", wide)).expect("恰好 128 个节点应编译成功");

    assert_eq!(compiled.nodes.len(), 128);
}

/// **AC #9（续）** —— 恰好 8 层嵌套**不**超限。
#[test]
fn exactly_the_depth_limit_is_accepted() {
    let mut subgraphs: Vec<(String, GraphDefinition)> = Vec::new();
    for level in 1..=8 {
        let child = if level == 8 {
            graph("leaf", vec![lambda("leaf_node", &[])])
        } else {
            graph(
                &format!("inner{level}"),
                vec![subgraph("n", &format!("s{}", level + 1), &[])],
            )
        };
        subgraphs.push((format!("s{level}"), child));
    }
    let refs: Vec<(&str, GraphDefinition)> = subgraphs
        .iter()
        .map(|(name, def)| (name.as_str(), def.clone()))
        .collect();
    let definition = graph_with_subgraphs("deep8", vec![subgraph("top", "s1", &[])], &refs);

    let compiled = compile(&definition).expect("恰好 8 层嵌套应编译成功");

    assert!(!compiled.nodes.is_empty());
}

/// **AC #10** —— 超限映射到 `ErrorCode::BudgetExceeded`（30006），**不是** panic。
#[test]
fn limit_violations_map_to_budget_exceeded_not_panic() {
    // 深度超限 → BudgetExceeded。
    let mut subgraphs: Vec<(String, GraphDefinition)> = Vec::new();
    for level in 1..=9 {
        let child = if level == 9 {
            graph("leaf", vec![lambda("leaf_node", &[])])
        } else {
            graph(
                &format!("inner{level}"),
                vec![subgraph("n", &format!("s{}", level + 1), &[])],
            )
        };
        subgraphs.push((format!("s{level}"), child));
    }
    let refs: Vec<(&str, GraphDefinition)> = subgraphs
        .iter()
        .map(|(name, def)| (name.as_str(), def.clone()))
        .collect();
    let definition = graph_with_subgraphs("deep9", vec![subgraph("top", "s1", &[])], &refs);

    let err = compile(&definition).expect_err("深度超限必须编译失败");
    let app: nexus_core::error::AppError = err.into();
    assert_eq!(
        app.user_facing().code,
        ErrorCode::BudgetExceeded,
        "深度超限应映射到 BudgetExceeded（30006），实际：{:?}",
        app.user_facing().code
    );
    assert_eq!(app.user_facing().code.code(), 30006, "码值应为 30006");
}

/// **AC #10（续）** —— 定义**非法**（环）映射到 `WorkflowDsl`，**不是** `BudgetExceeded`。
#[test]
fn invalid_definition_maps_to_workflow_dsl() {
    let definition = graph("cyclic", vec![lambda("a", &["b"]), lambda("b", &["a"])]);

    let err = compile(&definition).expect_err("有环的图必须编译失败");
    let app: nexus_core::error::AppError = err.into();

    assert_eq!(
        app.user_facing().code,
        ErrorCode::WorkflowDsl,
        "定义非法应映射到 WorkflowDsl，实际：{:?}",
        app.user_facing().code
    );
}

/// **AC #10（续）** —— 状态超限也映射到 `BudgetExceeded`。
#[test]
fn state_size_violation_maps_to_budget_exceeded() {
    let err = check_state_size(4 * 1024 * 1024 + 1).expect_err("超限必须报错");
    let app: nexus_core::error::AppError = err.into();

    assert_eq!(
        app.user_facing().code,
        ErrorCode::BudgetExceeded,
        "状态超限应映射到 BudgetExceeded，实际：{:?}",
        app.user_facing().code
    );
}

/// 悬空引用 —— 引用了不存在的节点 → 错误（不是 panic）。
#[test]
fn dangling_reference_is_rejected() {
    let definition = graph("dangling", vec![lambda("a", &["ghost"])]);

    let err = compile(&definition).expect_err("悬空引用必须编译失败");

    match err {
        CompileError::UnknownNode { node, missing } => {
            assert_eq!(node.as_str(), "a");
            assert_eq!(missing.as_str(), "ghost");
        }
        other => panic!("应报 UnknownNode，实际：{other:?}"),
    }
}

/// 引用了**不存在**的子图 → 错误。
#[test]
fn missing_subgraph_is_rejected() {
    let definition = graph("missing_sub", vec![subgraph("sg", "nope", &[])]);

    let err = compile(&definition).expect_err("引用不存在的子图必须失败");

    assert!(
        matches!(err, CompileError::UnknownNode { .. }),
        "应报 UnknownNode，实际：{err:?}"
    );
}

/// 节点键重复 → 错误。
#[test]
fn duplicate_node_id_is_rejected() {
    let definition = graph("dup", vec![lambda("a", &[]), lambda("a", &[])]);

    let err = compile(&definition).expect_err("重复节点键必须编译失败");

    assert!(
        matches!(err, CompileError::DuplicateNode { .. }),
        "应报 DuplicateNode，实际：{err:?}"
    );
}

/// 空图 → 错误。
#[test]
fn empty_graph_is_rejected() {
    let err = compile(&graph("empty", vec![])).expect_err("空图必须编译失败");

    assert!(
        matches!(err, CompileError::EmptyGraph),
        "应报 EmptyGraph，实际：{err:?}"
    );
}

/// 子图摊平后，容器的**后继**由**子图出口**把关 ——
///
/// ⚠️ 挂在容器上会让 `c` 在子图**跑完之前**就绪。
#[test]
fn parent_successor_is_gated_by_subgraph_exit_not_by_container() {
    let sub = graph("sub", vec![lambda("s1", &["s2"]), lambda("s2", &[])]);
    let definition = graph_with_subgraphs(
        "parent",
        vec![
            lambda("a", &["sg"]),
            subgraph("sg", "sub", &["c"]),
            lambda("c", &[]),
        ],
        &[("sub", sub)],
    );

    let compiled = compile(&definition).expect("合法的图应编译成功");

    let c = NodeId::new("c");
    let s2 = NodeId::new("sg/s2");

    // 容器完成了，但子图**还没**跑完 → c 不可触发。
    assert!(
        !compiled.is_ready(&c, &set(&["a", "sg"])),
        "子图未跑完时父图后继不得触发"
    );
    // 子图出口完成了 → c 可触发。
    assert!(
        compiled.is_ready(&c, &set(&["a", "sg", "sg/s1", "sg/s2"])),
        "子图出口完成后父图后继应可触发"
    );
    // 子图入口的前驱是**容器** —— 容器没跑完则入口不可触发。
    let s1 = NodeId::new("sg/s1");
    assert!(
        !compiled.is_ready(&s1, &set(&["a"])),
        "容器未完成时子图入口不得触发"
    );
    assert!(
        compiled.is_ready(&s1, &set(&["a", "sg"])),
        "容器完成后子图入口应可触发"
    );
    // 出口节点的键带**限定路径**（容器 id 前缀）。
    assert!(
        compiled.node(&s2).is_some(),
        "子图节点应带限定路径（sg/s2）"
    );
}
