#!/usr/bin/env bash
# 校验 Spec 层（AI-SDLC 三层模型 Tier 2）的组织纪律。
#
# 背景：
#   本仓库一度**只做 Task 层** —— 直接写 backlog 任务就开工，
#   Spec 层的 `spec.md` + `tasks.md` + `contracts/` 从未产出。
#   后果是「计划晚于执行」：任务拆解与实现同在一个提交里落盘，
#   没有可追溯的契约，也无法机械校验契约是否漂移。
#
# 本脚本把 Spec 层变成**可执行**的：
#   1. 每个 spec 目录必须有 spec.md + tasks.md + contracts/
#   2. tasks.md 的每个 T-NNN 必须有对应的 backlog 任务，且**双向**可解析
#   3. contracts/ 的不变量条目引用的**测试名必须真的存在**（防伪）
#   4. contracts/ 的错误码白名单必须与 nexus-core 的实际码值一致（防漂移）
#
# 用法：bash scripts/check-specs.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SPEC_ROOT="docs/specs"
fail=0

if [ ! -d "$SPEC_ROOT" ]; then
    echo "── 无 Spec 层目录 ──"
    echo "  ✗ $SPEC_ROOT 不存在"
    echo "      Spec 层是 Tier 2 —— 一个特性拆成多个任务时应先产出 Spec。"
    echo "      见 .cursor/rules/ai-sdlc-workflow.mdc §1。"
    exit 1
fi

# ── 规则 1：每个 spec 目录的三件套齐全 ──
echo "── 规则 1：Spec 三件套齐全（spec.md + tasks.md + contracts/） ──"
missing=0
for dir in "$SPEC_ROOT"/*/; do
    [ -d "$dir" ] || continue
    name="$(basename "$dir")"
    for f in spec.md tasks.md contracts; do
        if [ ! -e "$dir/$f" ]; then
            echo "  ✗ ${name}/ 缺少 ${f}"
            missing=$((missing + 1))
        fi
    done
done
if [ "$missing" -eq 0 ]; then
    echo "  ✓ 所有 spec 目录三件套齐全"
else
    fail=1
fi

echo ""
echo "── 规则 2：tasks.md 的 T-NNN 与 backlog 任务双向对应 ──"
# 双向对应是防「文档漂移」的关键：
#   - 只查 tasks.md → backlog：漏了「任务被删了但文档还在」
#   - 只查 backlog → tasks.md：漏了「任务没登记进 Spec」
unlinked=0
for dir in "$SPEC_ROOT"/*/; do
    [ -d "$dir" ] || continue
    name="$(basename "$dir")"
    tasks_file="$dir/tasks.md"
    [ -f "$tasks_file" ] || continue

    # 从 tasks.md 抽出 T-NNN 与对应的 backlog 任务号（NEX-nn）
    # 格式：对应 backlog 任务：`backlog/tasks/NEX-16 - ....md`
    t_ids=$(grep -oE '^### T-[0-9]+' "$tasks_file" | sed 's/^### //' || true)
    nex_ids=$(grep -oE 'NEX-[0-9]+' "$tasks_file" | sort -u || true)

    for t in $t_ids; do
        if ! echo "$nex_ids" | grep -qE '^NEX-'; then
            echo "  ✗ ${name}/${t}：tasks.md 没有任何 NEX 引用"
            unlinked=$((unlinked + 1))
        fi
    done

    # 每个登记的 NEX 任务必须真实存在于 backlog（tasks/ 或 completed/）
    for n in $nex_ids; do
        found=0
        for base in backlog/tasks backlog/completed; do
            if ls "$base"/"$n"*.md >/dev/null 2>&1; then
                found=1
                break
            fi
        done
        if [ "$found" -eq 0 ]; then
            echo "  ✗ ${name}：tasks.md 引用了 ${n}，但 backlog 里找不到该任务"
            unlinked=$((unlinked + 1))
        fi
    done

    # 每个 T-NNN 必须有可二元判定的 AC
    ac_count=$(grep -cE '^- AC: ' "$tasks_file" || true)
    t_count=$(echo "$t_ids" | grep -cE 'T-[0-9]+' || true)
    if [ "$t_count" -gt 0 ] && [ "$ac_count" -lt "$t_count" ]; then
        echo "  ✗ ${name}：${t_count} 个任务只有 ${ac_count} 条 AC —— 每个任务至少一条"
        unlinked=$((unlinked + 1))
    fi
done
if [ "$unlinked" -eq 0 ]; then
    echo "  ✓ tasks.md 与 backlog 双向对应"
else
    fail=1
fi

echo ""
echo "── 规则 3：contracts/ 的不变量引用的测试必须真的存在 ──"
# 这是「防伪」的关键 —— 契约说「某测试验证了这条不变量」，
# 但那个测试根本不存在时，契约就是**空头支票**。
#
# 做法：把 `verified_by` 的每一项解析成
#   <文件路径>::<测试名>
# 然后在对应文件里 grep 该测试名。文件路径可能带 `::` 形式的模块限定，
# 也可能是裸测试名（如 `crates/nexus-llm/src/retry.rs` 指向模块内全部测试）。
missing_tests=0
for f in "$SPEC_ROOT"/*/contracts/*.json; do
    [ -e "$f" ] || continue
    # 提取 verified_by 数组里的每一项
    entries=$(python3 - "$f" <<'PY'
import json, sys
with open(sys.argv[1]) as fh:
    doc = json.load(fh)
for inv in doc.get("invariants", []):
    for v in inv.get("verified_by", []):
        if "::" in v:
            print(v)
PY
)
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        path="${entry%%::*}"
        test_name="${entry##*::}"
        if [ ! -f "$path" ]; then
            echo "  ✗ $(basename "$f")：引用的文件不存在 → $path"
            missing_tests=$((missing_tests + 1))
            continue
        fi
        if ! grep -qE "fn[[:space:]]+$test_name\b" "$path"; then
            echo "  ✗ $(basename "$f")：测试不存在 → $entry"
            missing_tests=$((missing_tests + 1))
        fi
    done <<< "$entries"
done
if [ "$missing_tests" -eq 0 ]; then
    echo "  ✓ 契约引用的测试全部存在"
else
    fail=1
fi

echo ""
echo "── 规则 4：错误码白名单与 nexus-core 的实际码值一致 ──"
# P3 不得新增码值、不得改码值（架构文档 §6.3 已定稿）。
# 白名单与实际码值漂移时，说明有人改了契约或改了实现 —— 两者都需要显式承认。
drift=0
for f in "$SPEC_ROOT"/*/contracts/error-codes.json; do
    [ -e "$f" ] || continue
    # python 失败时也要继续（`set -e` 下命令替换的失败会中断脚本）——
    # 结果通过 stdout 传回，问题条数靠「退出码」判断。
    result=$(python3 - "$f" <<'PY' || true
import json, re, sys

with open(sys.argv[1]) as fh:
    doc = json.load(fh)

src = open(doc["source_of_truth"]).read()

# ⚠️ 必须**只**解析 `code()` 函数体 ——
# `Self::Variant => 12345,` 这个形状在整个 impl 块里出现三次：
# `code()`、`http_status()`、`retry_policy()`。
# 用全文正则会把 HTTP 状态码（429/500/503）当成业务码抓进来。
m = re.search(
    r"pub const fn code\(self\)\s*->\s*i32\s*\{(.*?)\n    \}",
    src,
    re.S,
)
if not m:
    print("无法在源文件里定位 `code()` 函数体")
    sys.exit(0)

actual = dict(re.findall(r"Self::(\w+)\s*=>\s*(\d+)\s*,", m.group(1)))

problems = []
for entry in doc["whitelist"]:
    variant, code = entry["variant"], str(entry["code"])
    if variant not in actual:
        problems.append(f"白名单的 {variant} 在 {doc['source_of_truth']} 里不存在")
    elif actual[variant] != code:
        problems.append(
            f"{variant} 码值漂移：契约写 {code}，实际 {actual[variant]}"
        )

for p in problems:
    print(p)
PY
)
    if [ -n "$result" ]; then
        echo "$result" | sed 's/^/  ✗ /'
        drift=$((drift + 1))
    fi
done
if [ "$drift" -eq 0 ]; then
    echo "  ✓ 错误码白名单与 nexus-core 一致"
else
    fail=1
fi

echo ""
echo "── 规则 5：nexus-llm 只使用白名单内的错误码 ──"
# 新增码值是 §6.3 已定稿条款的破坏性变更 —— 必须在 Spec 契约里显式登记，
# 而不是悄悄出现在实现里。
used_outside=0
whitelist_variants=$(python3 - "$SPEC_ROOT"/*/contracts/error-codes.json <<'PY'
import json, sys
variants = set()
for path in sys.argv[1:]:
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError):
        continue
    for entry in doc.get("whitelist", []):
        variants.add(entry["variant"])
print("\n".join(sorted(variants)))
PY
)
for crate in crates/nexus-llm; do
    [ -d "$crate/src" ] || continue
    # ⚠️ 必须**剥离注释**再扫描 ——
    # 文档注释里解释「委托给 `nexus-core::ErrorCode::retry_policy()`」
    # 是正常文字，不是「使用了这个错误码」。
    #
    # 这与 scripts/check-deps.sh 规则 6b 是同一个坑（那里踩过一次）。
    used=$(find "$crate/src" -name '*.rs' -type f -print0 \
        | xargs -0 sed -e 's://.*::' \
        | grep -ohE 'ErrorCode::[A-Za-z]+' 2>/dev/null \
        | sed 's/ErrorCode:://' | sort -u || true)
    for v in $used; do
        if ! echo "$whitelist_variants" | grep -qx "$v"; then
            echo "  ✗ nexus-llm 使用了白名单外的错误码：$v"
            used_outside=$((used_outside + 1))
        fi
    done
done
if [ "$used_outside" -eq 0 ]; then
    echo "  ✓ nexus-llm 未使用白名单外的错误码"
else
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ Spec 层组织纪律检查全部通过"
else
    echo "✗ 存在 Spec 层纪律问题"
fi
exit "$fail"
