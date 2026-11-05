#!/usr/bin/env bash
# 校验 backlog 的组织纪律。
#
# 检查项：
#   1. 同一任务不得同时出现在 backlog/tasks/ 与 backlog/completed/
#      —— 归档用 `git mv`，但编辑器可能在旧路径重建同名文件（见 NEX-7 的教训）
#   2. backlog/completed/ 下的任务文件 front-matter 必须标记为已完成
#   3. 任务文件必须包含 `## DoR 判定` 段落（豁免名单除外）
#
# 用法：bash scripts/check-backlog.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

echo "── 1. 检查任务是否同时存在于 tasks/ 与 completed/ ──"
dupes=0
if [ -d backlog/tasks ] && [ -d backlog/completed ]; then
    for f in backlog/tasks/*.md; do
        [ -e "$f" ] || continue
        name="$(basename "$f")"
        if [ -e "backlog/completed/$name" ]; then
            echo "  ✗ 重复：$name 同时存在于 tasks/ 与 completed/"
            echo "      归档用 git mv；若被误留，请 git rm 掉 tasks/ 下的那一份"
            dupes=$((dupes + 1))
        fi
    done
fi
if [ "$dupes" -eq 0 ]; then
    echo "  ✓ 没有重复的任务文件"
else
    echo "  ✗ 发现 $dupes 个重复的任务文件"
    fail=1
fi

echo ""
echo "── 2. 检查 completed/ 下的任务是否标记为已完成 ──"
unmarked=0
for f in backlog/completed/*.md; do
    [ -e "$f" ] || continue
    if ! grep -qE '^status:[[:space:]]*(Done|Completed|已归档)' "$f"; then
        echo "  ✗ $(basename "$f") 的 status 未标记完成"
        unmarked=$((unmarked + 1))
    fi
done
if [ "$unmarked" -eq 0 ]; then
    echo "  ✓ 已归档任务均标记为完成"
else
    echo "  ✗ 发现 $unmarked 个未标记完成的任务文件"
    fail=1
fi

echo ""
echo "── 3. 检查任务文件含 DoR 判定记录 ──"
# 为什么需要这条
#
# `ai-sdlc-dor-gate.mdc` §5 要求「判定记录必须落到任务文件里，
# 否则下次接手的人无法复核」。
#
# 为什么用**机器校验**而不是规则措辞：
# 本仓库的规则措辞已经被验证过会**被绕过** —— 早期 18 个任务
# （NEX-2 ~ NEX-18）完成于这条要求写进规则**之前**，全都**没有**判定记录，
# 而**当时没有任何东西拦住它们**。
#
# 「看起来覆盖了，实际没有」在**流程层**的又一次重演。
#
# ⚠️ 豁免名单是**封闭**的：新增条目必须走决策记录。
# 理由 —— 豁免名单一旦可以随意加，就变成了「漏写就加一条豁免」的后门，
# 那这条门禁就等于没有。
#
# 豁免理由：这些任务完成于「判定记录必须落盘」写进规则**之前**。
# **不追溯补写** —— 补写等于伪造从未发生过的审计条目
# （`ai-sdlc-observability.mdc` §3.3：审计条目一旦写入即不可变，
# 更正通过新增条目完成；这里根本没有原始条目可更正）。
DOR_EXEMPT_IDS="NEX-2 NEX-3 NEX-4 NEX-5 NEX-6 NEX-7 NEX-8 NEX-9 NEX-10 NEX-11 NEX-12 NEX-13 NEX-14 NEX-15 NEX-16 NEX-17 NEX-18"

missing_dor=0
for f in backlog/tasks/*.md backlog/completed/*.md; do
    [ -e "$f" ] || continue
    name="$(basename "$f")"

    # 提取任务号（NEX-<n>），用于比对豁免名单。
    task_id="$(printf '%s' "$name" | sed -n 's/^\(NEX-[0-9][0-9]*\).*/\1/p')"
    if [ -n "$task_id" ] && printf '%s' " $DOR_EXEMPT_IDS " | grep -q " $task_id "; then
        continue
    fi

    # 段落标题必须**恰好**是 `## DoR 判定`（允许行尾空白）。
    # 用 `^##[[:space:]]+DoR[[:space:]]+判定` 匹配，避免漏掉标题变体。
    if ! grep -qE '^##[[:space:]]+DoR[[:space:]]+判定' "$f"; then
        echo "  ✗ $name 缺少 \`## DoR 判定\` 段落"
        echo "      判定记录必须落到任务文件里，否则下次接手的人无法复核"
        echo "      写不出七项判定 = DoR 未过 → 标 \`status: Needs Clarification\` 并写明澄清项"
        missing_dor=$((missing_dor + 1))
    fi
done
if [ "$missing_dor" -eq 0 ]; then
    echo "  ✓ 全部任务文件均含 DoR 判定记录（${DOR_EXEMPT_IDS} 依豁免名单跳过）"
else
    echo "  ✗ 发现 $missing_dor 个缺少 DoR 判定记录的任务文件"
    fail=1
fi

echo ""
echo "── 3b. 检查豁免名单本身没有后门 ──"
# 为什么需要这条
#
# 规则 3 的豁免名单有一个**天然的后门**：
# 先把未来任务号写进名单，等任务文件创建出来时，它就**自动被豁免**了。
#
# 故障注入实测过：把 `NEX-99` 加进名单后再创建 `NEX-99` 的任务文件 ——
# 门禁**全绿**，缺失的判定记录**没有被报出来**。
#
# 那等于豁免名单变成了「漏写就加一条」的自动后门 ——
# 这条门禁就等于没有（正是我们想避免的）。
#
# 所以豁免名单必须满足两条：
#   1. 每个 ID 都**已存在**于 backlog/ 下（不能给还不存在的任务预留）；
#   2. 每个被豁免的文件**确实**缺少判定记录
#      （否则豁免是**多余的**，应当移除 —— 避免名单只增不减）。
stale_exempt=0
for id in $DOR_EXEMPT_IDS; do
    # 找到该 ID 对应的任务文件（tasks/ 或 completed/）。
    #
    # ⚠️ 匹配必须要求 ID **完整** ——
    # `NEX-2` 不能匹配到 `NEX-24 ...`！
    # 用 `"$id"-*` 或 `"$id "*`（文件名的分隔符是空格或短横线），
    # 而不是 `"$id"*`（那会前缀匹配到 NEX-24 / NEX-20 / NEX-21 ...）。
    #
    # 规则 3b 第一次跑时就踩了这个：它把 NEX-24 误判成 NEX-2，
    # 然后报「NEX-2 已有 DoR 判定段落」—— 一个**假阳性**。
    matched=""
    for f in backlog/tasks/*.md backlog/completed/*.md; do
        [ -e "$f" ] || continue
        base="$(basename "$f")"
        case "$base" in
            "$id"-*|"$id "*) matched="$f"; break ;;
        esac
    done

    if [ -z "$matched" ]; then
        echo "  ✗ 豁免名单里的 ${id} 在 backlog 下**不存在**对应任务文件"
        echo "      豁免名单不能给「还不存在的任务」预留 —— 那等于自动后门"
        stale_exempt=$((stale_exempt + 1))
        continue
    fi

    # 被豁免的文件必须**确实**缺判定记录，否则豁免是多余的。
    if grep -qE '^##[[:space:]]+DoR[[:space:]]+判定' "$matched"; then
        echo "  ✗ 豁免名单里的 ${id}（$(basename "$matched")）**已有** \`## DoR 判定\` 段落"
        echo "      豁免已多余，应当从名单里移除 —— 避免名单只增不减"
        stale_exempt=$((stale_exempt + 1))
    fi
done
if [ "$stale_exempt" -eq 0 ]; then
    echo "  ✓ 豁免名单无后门（每个条目都对应真实存在且确实缺记录的历史任务）"
else
    echo "  ✗ 发现 $stale_exempt 处豁免名单问题"
    fail=1
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "✓ 全部检查通过"
else
    echo "✗ 存在检查失败项"
fi
exit "$fail"
