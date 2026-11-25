#!/bin/sh
# The discr coordination loop: one bead, from `bd ready` to a proposed MR.
#
# This is the workflow ten agent waves converged on, made executable. Every
# stage exists because skipping it cost a run:
#
#   0 preflight  the container is ephemeral; rebuild it, and settle the beads
#                database BEFORE reading work from it (a stale dolt conflict
#                silently blocks every later `bd` write).
#   1 orient     read the inbox and the lease table before touching a file.
#                A peer planning against the same path renegotiates now,
#                cheaply, instead of at merge time when both plans are sunk.
#   2 claim      the bead first (so `bd ready` stops offering it to peers),
#                then ONE all-or-nothing lease over every path the work will
#                write -- never a half-held set.
#   3 work       bernstein orchestrates the agents; the gates below decide
#                whether their output is real. No model sits in the
#                coordination loop itself, so a replay is byte-identical.
#   4 verify     the gates ARE the acceptance criteria. A number that shrinks
#                is a regression even when every test passes.
#   5 land       commit with explicit pathspecs and the agent's own trailer,
#                close the bead, export the beads (this container cannot push
#                refs/dolt/data), release the leases AFTER committing.
#   6 propose    an MR, automatically.
#
#   sh scripts/loop.sh --bead discr-tun --files "crates/disc-core/src/player.rs"
#   sh scripts/loop.sh --dry-run          # plan only: no agents, no writes
#   sh scripts/loop.sh --no-agent         # you do the work; the loop does the rest
#
# Identity comes from PACT_AGENT (pact never guesses one) and BEADS_ACTOR must
# match it, or bd attributes the whole fleet's task tracking to whoever owns
# the checkout's git identity.
set -eu

cd "$(dirname "$0")/.."
ROOT=$(pwd)

BEAD=""
FILES=""
GOAL=""
DRY_RUN=0
USE_AGENT=1
BUDGET=${BUDGET:-10}
BASE=${BASE:-master}

usage() {
    sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --bead)     BEAD=$2; shift 2 ;;
        --files)    FILES=$2; shift 2 ;;
        --goal)     GOAL=$2; shift 2 ;;
        --budget)   BUDGET=$2; shift 2 ;;
        --base)     BASE=$2; shift 2 ;;
        --dry-run)  DRY_RUN=1; shift ;;
        --no-agent) USE_AGENT=0; shift ;;
        -h|--help)  usage 0 ;;
        *)          echo "loop: unknown argument: $1" >&2; usage 2 ;;
    esac
done

say()  { printf '\n=== %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
die()  { printf 'loop: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# bd is ALWAYS run against the main checkout: a worktree without -C silently
# initialises a second database.
bd_() { bd -C "$ROOT" "$@"; }

# ---- 0. preflight --------------------------------------------------------
say "0. preflight"

[ -n "${PACT_AGENT:-}" ] || die "set PACT_AGENT first (pact never guesses an identity); export BEADS_ACTOR=\$PACT_AGENT too"
[ "${BEADS_ACTOR:-}" = "$PACT_AGENT" ] || die "BEADS_ACTOR (${BEADS_ACTOR:-unset}) must equal PACT_AGENT ($PACT_AGENT), or bd attributes this work to the checkout's git identity"
note "agent: $PACT_AGENT"

if [ -x scripts/setup_container.sh ] && [ "${SKIP_SETUP:-0}" != 1 ]; then
    sh scripts/setup_container.sh >/dev/null 2>&1 || note "setup_container: partial (continuing)"
fi
have bd   || die "bd not found -- run scripts/setup_container.sh"
have pact || die "pact not found -- run scripts/setup_container.sh"

# A dolt conflict left by a previous sync blocks every subsequent bd write with
# an error that names neither the loop nor the bead. Settle it up front.
if bd_ sql -q "select count(*) from dolt_conflicts" 2>/dev/null | grep -qE '^\s*[1-9]'; then
    note "beads: conflicts present -- resolving in favour of the remote (canonical)"
    bd_ sql -q "call dolt_conflicts_resolve('--theirs', 'issues', 'child_counters')" >/dev/null 2>&1 || true
    bd_ dolt commit -m "merge: resolve conflicts theirs (loop preflight)" >/dev/null 2>&1 || true
fi
bd_ dolt pull >/dev/null 2>&1 && note "beads: pulled refs/dolt/data" \
    || note "beads: pull skipped (no remote data, or offline)"

# ---- 1. orient -----------------------------------------------------------
say "1. orient"
pact msg inbox 2>/dev/null | head -12 || true
note "--- leases held right now ---"
pact lease ls 2>/dev/null | head -12 || true

if [ -z "$BEAD" ]; then
    note "--- ready work ---"
    bd_ ready 2>/dev/null | grep -E '^[○◐]' | head -10 || true
    BEAD=$(bd_ ready --json 2>/dev/null \
           | python3 -c 'import json,sys
rows=[i for i in json.load(sys.stdin) if not i.get("issue_type")=="epic"]
rows.sort(key=lambda i: i.get("priority", 9))
print(rows[0]["id"] if rows else "")' 2>/dev/null || true)
    [ -n "$BEAD" ] || die "no ready bead found -- pass --bead <id>"
    note "selected highest-priority non-epic bead: $BEAD"
fi

bd_ show "$BEAD" >/dev/null 2>&1 || die "no such bead: $BEAD"
# `bd show --json` answers with a LIST of one issue, not the issue -- reading it
# as an object silently yields the bead id as its own title, which then becomes
# the MR title.
TITLE=$(bd_ show "$BEAD" --json 2>/dev/null \
        | python3 -c 'import json,sys
d = json.load(sys.stdin)
d = d[0] if isinstance(d, list) and d else d
print(d.get("title", "") if isinstance(d, dict) else "")' 2>/dev/null || true)
[ -n "$TITLE" ] || TITLE=$BEAD
note "bead:  $BEAD"
note "title: $TITLE"

if [ "$DRY_RUN" = 1 ]; then
    say "dry run: stopping before any claim, lease or write"
    [ "$USE_AGENT" = 1 ] && have bernstein && \
        bernstein --plan-only -g "${GOAL:-$TITLE}" 2>&1 | tail -20 || true
    exit 0
fi

# ---- 2. claim ------------------------------------------------------------
say "2. claim"
# --assignee, not --claim: on bd 1.2.2 `--claim` writes no interaction row, so
# `pact audit --check claim-lease-divergence` has nothing to read afterwards.
bd_ update "$BEAD" --assignee="$PACT_AGENT" --status in_progress >/dev/null
note "bd: $BEAD assigned to $PACT_AGENT, in_progress"

if [ -n "$FILES" ]; then
    # One acquire, all paths: several paths in one call are taken
    # all-or-nothing, so the loop never holds half of what it needs while a
    # peer holds the rest. --wait blocks INSIDE the command: ending a turn to
    # wait is the same as exiting, and a subagent that exits never resumes.
    # shellcheck disable=SC2086
    if pact lease acquire $FILES --wait 30m --note "$BEAD: $TITLE"; then
        note "pact: leased $FILES"
    else
        die "could not acquire leases for: $FILES (see 'pact lease ls' for the holder)"
    fi
else
    note "pact: no --files given; lease before writing anything shared"
fi

# ---- 3. work -------------------------------------------------------------
say "3. work"
if [ "$USE_AGENT" = 1 ] && have bernstein; then
    PLAN=plans/discr-loop.yaml
    if [ -f "$PLAN" ]; then
        note "bernstein: running $PLAN (budget \$$BUDGET, approval=pr)"
        bernstein run "$PLAN" --budget "$BUDGET" --approval pr --auto-approve || \
            note "bernstein: returned non-zero -- the gates below are the arbiter"
    else
        note "bernstein: inline goal (budget \$$BUDGET, approval=pr)"
        bernstein -g "${GOAL:-$TITLE}" --budget "$BUDGET" --approval pr --auto-approve || \
            note "bernstein: returned non-zero -- the gates below are the arbiter"
    fi
else
    note "no-agent mode: do the work now, then re-run with --no-agent to land it"
fi

# ---- 4. verify -----------------------------------------------------------
say "4. verify (the gates are the acceptance criteria)"
GATES_OK=1
if have mise; then
    mise run core-check || GATES_OK=0
else
    note "mise absent -- running the gate suite directly"
    cargo fmt --check                             || GATES_OK=0
    cargo clippy --all-targets -- -D warnings     || GATES_OK=0
    cargo test                                    || GATES_OK=0
fi
if [ "$GATES_OK" != 1 ]; then
    bd_ comment "$BEAD" "loop($PACT_AGENT): gates FAILED; nothing landed. Leases still held." >/dev/null 2>&1 || true
    die "gates failed -- fix, then re-run. Nothing was committed and no lease was released."
fi
note "gates: green"

# ---- 5. land -------------------------------------------------------------
say "5. land"
if [ -n "$(git status --porcelain)" ]; then
    # Explicit pathspecs. A bare `git commit` commits the whole INDEX, which in
    # a shared checkout sweeps in whatever a peer had staged -- one run put
    # another agent's staged deletion into an unrelated commit. With --files
    # the loop commits exactly what it leased; without, it commits its own
    # checkout wholesale and says so.
    if [ -n "$FILES" ]; then
        PATHSPEC=$FILES
    else
        PATHSPEC=.
        note "no --files: committing this checkout wholesale (lease next time)"
    fi
    # shellcheck disable=SC2086
    git add -- $PATHSPEC >/dev/null
    # shellcheck disable=SC2086
    git commit --trailer "Pact-Agent=$PACT_AGENT" -m "$(printf '%s\n\n%s\n\n%s\n%s\n' \
        "feat($BEAD): $TITLE" \
        "Landed by the discr coordination loop (scripts/loop.sh) with all gates green." \
        "Co-Authored-By: Claude <noreply@anthropic.com>" \
        "Claude-Session: ${CLAUDE_SESSION_URL:-https://claude.ai/code}")" -- $PATHSPEC >/dev/null
    note "committed $(git rev-parse --short HEAD)"
else
    note "nothing to commit (work may already be committed)"
fi

bd_ comment "$BEAD" "loop($PACT_AGENT): gates green at $(git rev-parse --short HEAD); see the MR." >/dev/null 2>&1 || true
# The container's git proxy accepts pushes to branch heads only, so
# `bd dolt push` 403s: the JSONL export is how bead changes leave this box.
bd_ export -o .beads/issues.jsonl >/dev/null 2>&1 && git add .beads/issues.jsonl 2>/dev/null || true
git add .pact/events.jsonl .pact/messages.jsonl .beads/interactions.jsonl 2>/dev/null || true
git diff --cached --quiet || git commit --trailer "Pact-Agent=$PACT_AGENT" \
    -m "chore(pact): ledger checkpoint -- $BEAD" >/dev/null 2>&1 || true

# Commit BEFORE releasing: a lease released while the work is uncommitted
# breaks the one binding `pact audit --check commit-correlation` exists to prove.
pact lease release --all >/dev/null 2>&1 && note "pact: leases released" || true
pact msg send --to orchestrator \
    "$PACT_AGENT: $BEAD landed at $(git rev-parse --short HEAD), gates green" >/dev/null 2>&1 || true

BRANCH=$(git branch --show-current)
git push -u origin "$BRANCH" >/dev/null 2>&1 && note "pushed $BRANCH" \
    || note "push failed -- retry before proposing the MR"

# ---- 6. propose the MR ---------------------------------------------------
say "6. propose the MR"
mkdir -p .sdd/mr
MR=.sdd/mr/$BRANCH.md
{
    printf '# %s\n\n' "$TITLE"
    printf '**Bead:** `%s` — landed by `scripts/loop.sh` as `%s`.\n\n' "$BEAD" "$PACT_AGENT"
    printf '## What\n\n'
    bd_ show "$BEAD" 2>/dev/null | sed -n '/DESCRIPTION/,/ACCEPTANCE\|PARENT\|NOTES/p' | sed 's/^ \{0,2\}//'
    printf '\n## Commits\n\n```\n'
    git log --oneline "origin/$BASE..$BRANCH" 2>/dev/null | head -20
    printf '```\n\n## Validation\n\nAll gates green (`mise run core-check`) at `%s`.\n' "$(git rev-parse --short HEAD)"
} > "$MR"
note "MR body: $MR"

if have gh; then
    gh pr create --base "$BASE" --head "$BRANCH" --title "$TITLE" --body-file "$MR" \
        && note "MR opened" || note "gh failed -- open the MR from $MR"
else
    # No gh in this container by design; the orchestrator opens the MR through
    # the GitHub MCP with exactly this body.
    note "no gh CLI here -- MR is PROPOSED, not opened:"
    note "  head=$BRANCH  base=$BASE  title=$TITLE"
    note "  body=$MR"
fi

say "loop complete: $BEAD -> $(git rev-parse --short HEAD) -> MR proposed"
