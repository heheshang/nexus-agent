---
id: NEX-3
title: "feat(config): 分层配置骨架与 NX_ 环境变量映射表"
status: To Do
assignee: []
labels: [config, p0]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.7"
---

## Description

架构文档 §6.2 把「配置骨架（分层配置 + `NX_` 环境变量映射表）」列为 P0 任务，目前 `nexus-config` 只有 7 行占位，仓库也没有 `configs/` 目录。

架构文档 §2.7 定稿的设计要点：

- 分层配置：`base`（打包进二进制的默认值）→ `file`（`configs/*.yaml`）→ `env`（`NX_` 前缀环境变量）
- 环境变量格式：`NX_<SECTION>__<KEY>`，层级用 `__` 分隔（如 `NX_LLM__DEEPSEEK__BASE_URL`）
- 环境变量覆盖由**声明式映射表**驱动，**不是** `set_var` 逐个覆盖 —— 这一点与参考实现（viper 自动匹配）不同，是刻意的显式化设计
- 哪些环境变量能覆盖什么，必须**可枚举、可文档化**

`nexus-config` 当前已有依赖：`serde` / `serde_json` / `serde_yaml_ng` / `toml` / `config` / `dotenvy` / `secrecy` / `chrono`。

## Acceptance Criteria

- [ ] #1 `ConfigStack` 类型按 `base` → `file` → `env` 三层实现，优先级从低到高，且**层级来源在类型上可区分**（不是三个同类型 `Config` 的隐式合并）
- [ ] #2 存在**声明式映射表**（不是 `set_var` 逐个覆盖），能把 `NX_A__B` 形式的环境变量映射到配置路径
- [ ] #3 存在一份**可枚举**的映射清单（表本身或生成的文档），能回答「哪些环境变量能覆盖什么」
- [ ] #4 仓库存在 `configs/` 目录，含示例配置文件；`configs/local.yaml` 与 `configs/secrets.yaml` 已在 `.gitignore` 中且确实未被入库
- [ ] #5 环境变量覆盖有单元测试：至少覆盖「未设置时不覆盖」「设置后覆盖生效」「未知环境变量被忽略或报错」三种情形
- [ ] #6 `cargo test --workspace --locked` 通过
- [ ] #7 `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #8 CI 五道门禁全绿
