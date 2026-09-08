#!/bin/bash
# Run every tests/scheme/*.scm under the external implementations and hold the
# differences to the register — #193 Phase 3.
#
#   ./scripts/run_suite_oracles.sh              # every file, every oracle found
#   ./scripts/run_suite_oracles.sh data/        # only files under data/
#   ./scripts/run_suite_oracles.sh --list       # print what the oracles answer
#                                               # and exit 0 whatever it is —
#                                               # for triage, and for writing
#                                               # new register rows
#
# Environment:
#   SUITE_ORACLE_TIMEOUT   seconds per file per oracle (default 60)
#   CHIBI / GOSH           override the interpreter binaries
#
# WHAT THIS CHECKS, AND WHAT IT DELIBERATELY DOES NOT
#
# It checks that the set of rows each oracle answers differently is exactly the
# set in `crates/patina-tests/tests/scheme/DIVERGENCES.tsv`. It does NOT check
# the oracles' pass/fail tallies. That distinction is the whole design:
#
#   - Gating on tallies makes an oracle's *bugfix* break our build, and the
#     natural repair is to edit a number — which trains mechanical updating and
#     says nothing about which side moved.
#   - Gating on the classified divergence set asks a better question: did a
#     difference appear that nobody has explained, or did an explained one go
#     away? Both deserve a human; neither is answered by a count.
#
# The tallies are still printed, because they are useful to a person and
# useless to a build.
#
# Nothing here can change what the suite asserts. The `.scm` rows assert
# Patina's answer; this lane only compares. An oracle that is wrong shows up as
# an `oracle-defect` row in the register, not as pressure on a test.
#
# Exit status: non-zero if a present oracle's divergences do not match the
# register, or if no oracle could be found at all. A missing oracle is reported
# loudly and skipped — silence would let a lane pass by running nothing, which
# is the failure `run_chibi_tests.sh` pins its total against.

set -eo pipefail

cd "$(dirname "$0")/.."

# Colour and the progress line are for a person watching. Redirected — a log,
# a CI job, the `> lane.log` someone reaches for when a run is slow — they are
# carriage returns, erase padding and escape sequences in a file nobody can
# read. The lane is a candidate for CI, where output is always redirected.
if [ -t 1 ]; then
    RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
    DIM='\033[2m'; NC='\033[0m'; INTERACTIVE=1
else
    RED=''; GREEN=''; YELLOW=''; DIM=''; NC=''; INTERACTIVE=0
fi

SUITE_DIR="crates/patina-tests/tests/scheme"
REGISTER="$SUITE_DIR/DIVERGENCES.tsv"
TIMEOUT="${SUITE_ORACLE_TIMEOUT:-60}"
CHIBI="${CHIBI:-chibi-scheme}"
GOSH="${GOSH:-gosh}"

LIST_ONLY=0
FILTER=""
for arg in "$@"; do
    case "$arg" in
        --list) LIST_ONLY=1 ;;
        --help|-h) sed -n '2,/^$/p' "$0" | sed -e 's/^#//' -e 's/^ //'; exit 0 ;;
        -*) echo "unknown option: $arg (try --help)" >&2; exit 2 ;;
        *) FILTER="$arg" ;;
    esac
done

[ -f "$REGISTER" ] || { echo "missing register: $REGISTER" >&2; exit 2; }

# Oracle invocations are arrays, not strings. A packed "gosh -r7" works in bash
# and silently does not in zsh, which does not word-split unquoted parameters —
# it exec's a file literally named "gosh -r7", produces no output and exits 0.
# That reads exactly like "the oracle ran and the file printed nothing", which
# is why the empty-output case below is an error rather than a shrug.
ORACLES=()
if command -v "$CHIBI" >/dev/null 2>&1; then
    ORACLES+=("chibi")
else
    echo -e "${YELLOW}chibi not found ($CHIBI) — its rows are not checked.${NC}"
    echo -e "${DIM}  brew install chibi-scheme${NC}"
fi
if command -v "$GOSH" >/dev/null 2>&1; then
    ORACLES+=("gauche")
else
    echo -e "${YELLOW}Gauche not found ($GOSH) — its rows are not checked.${NC}"
    echo -e "${DIM}  brew install gauche${NC}"
fi
if [ ${#ORACLES[@]} -eq 0 ]; then
    echo -e "${RED}No oracle found. This lane has nothing to run.${NC}" >&2
    exit 2
fi

# Run one file under one oracle, with a portable timeout. `perl -e 'alarm'` is
# the idiom run_larceny_tests.sh already uses; macOS has no coreutils timeout.
run_oracle() {
    local oracle=$1 file=$2
    case "$oracle" in
        chibi)  perl -e 'alarm shift; exec @ARGV' "$TIMEOUT" "$CHIBI" "$file" 2>&1 </dev/null || true ;;
        gauche) perl -e 'alarm shift; exec @ARGV' "$TIMEOUT" "$GOSH" -r7 "$file" 2>&1 </dev/null || true ;;
    esac
}

# SRFI 64 writes <suite>.log into the working directory. Run from a scratch dir
# so a lane never litters the repo — the same reason the Rust driver installs
# the null runner.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
REPO=$(pwd)

# Prove each oracle actually runs SRFI 64 before trusting a single result from
# it. Without this, an oracle that produces nothing at all is indistinguishable
# from one running a file that legitimately dies — and the files registered as
# `incomplete` expect exactly no output, so they would report green against a
# broken interpreter. Measured: `CHIBI=true ./scripts/run_suite_oracles.sh
# control/wind-thunk` passed and exited 0 before this existed.
cat >"$WORK/smoke.scm" <<'SMOKE'
(import (scheme base) (srfi 64))
(test-begin "smoke")
(test-equal 1 1)
(test-end)
SMOKE
for oracle in "${ORACLES[@]}"; do
    if ! (cd "$WORK" && run_oracle "$oracle" "$WORK/smoke.scm") \
        | grep -q '# of expected passes'; then
        echo -e "${RED}$oracle cannot run a three-line SRFI 64 program.${NC}" >&2
        echo -e "${DIM}  Every result from it would be meaningless, so this lane stops here${NC}" >&2
        echo -e "${DIM}  rather than reporting the files it expects to fail as passing.${NC}" >&2
        exit 2
    fi
done

registered_rows() {  # $1=file $2=oracle -> the rows the register expects
    awk -F'\t' -v f="$1" -v o="$2" \
        '!/^#/ && NF>=5 && $5!="" && $1==f && $2==o { print $3 }' "$REPO/$REGISTER"
}
registered_class() {  # $1=file $2=oracle $3=row
    awk -F'\t' -v f="$1" -v o="$2" -v r="$3" \
        '!/^#/ && NF>=5 && $5!="" && $1==f && $2==o && $3==r { print $4 }' "$REPO/$REGISTER"
}

FILES=$(cd "$SUITE_DIR" && find . -name '*.scm' | sed 's|^\./||' | sort)
[ -n "$FILTER" ] && FILES=$(echo "$FILES" | grep -F -- "$FILTER" || true)
[ -n "$FILES" ] || { echo "no suite files match '$FILTER'" >&2; exit 2; }

problems=0
checked=0

for f in $FILES; do
    for oracle in "${ORACLES[@]}"; do
        # Printed before the run, not after: two chibi pairs loop forever and
        # cost the whole timeout, and a line that only appears on completion
        # makes a 60 s wait look like a hang with nothing to blame it on.
        [ "$INTERACTIVE" = 1 ] && printf "  %-42s %-7s ..." "$f" "$oracle"
        out=$(cd "$WORK" && run_oracle "$oracle" "$REPO/$SUITE_DIR/$f")
        [ "$INTERACTIVE" = 1 ] && printf "\r%*s\r" 72 ""
        pass=$(echo "$out" | grep -oE '# of expected passes +[0-9]+' | grep -oE '[0-9]+$' || true)
        fails=$(echo "$out" | grep -oE '# of unexpected failures +[0-9]+' | grep -oE '[0-9]+$' || true)
        skips=$(echo "$out" | grep -oE '# of skipped tests +[0-9]+' | grep -oE '[0-9]+$' || true)
        actual=$(echo "$out" | grep -E '^FAIL ' | sed 's/^FAIL //' || true)

        expected=$(registered_rows "$f" "$oracle")
        expects_incomplete=0
        echo "$expected" | grep -qx '\*' && expects_incomplete=1

        if [ -z "$pass" ]; then
            # No SRFI 64 summary. Either the file legitimately dies here (and
            # the register says so), or the oracle produced nothing at all —
            # which is an invocation failure wearing the same clothes.
            if [ "$expects_incomplete" = 1 ] || [ "$LIST_ONLY" = 1 ]; then
                printf "  %-42s %-7s ${DIM}does not complete${NC}\n" "$f" "$oracle"
                checked=$((checked + 1))
            elif [ -z "$out" ]; then
                printf "  %-42s %-7s ${RED}ORACLE PRODUCED NO OUTPUT${NC}\n" "$f" "$oracle"
                echo -e "      ${DIM}not the same as 'the file died' — check the interpreter invocation${NC}"
                problems=$((problems + 1))
            else
                printf "  %-42s %-7s ${RED}did not complete, and is not registered${NC}\n" "$f" "$oracle"
                echo "$out" | tail -3 | sed 's/^/      /'
                problems=$((problems + 1))
            fi
            continue
        fi

        if [ "$expects_incomplete" = 1 ]; then
            printf "  %-42s %-7s ${RED}completes now, but is registered as incomplete${NC}\n" "$f" "$oracle"
            echo -e "      ${DIM}the oracle improved, or the file changed — retire the '*' row${NC}"
            problems=$((problems + 1))
            continue
        fi

        # The comparison the whole lane exists for: the set of differing rows,
        # not how many there are.
        new=$(comm -13 <(echo "$expected" | sort) <(echo "$actual" | sort))
        gone=$(comm -23 <(echo "$expected" | sort) <(echo "$actual" | sort))

        # --list reports and never judges: it exists for triage and for
        # writing new register rows, which is exactly when the rows do not
        # match yet. Checking there would refuse to show what you came to see.
        if [ "$LIST_ONLY" = 1 ]; then
            printf "  %-42s %-7s ${DIM}pass=%s fail=%s skip=%s${NC}\n" \
                "$f" "$oracle" "$pass" "${fails:-0}" "${skips:-0}"
            echo "$actual" | grep -v '^$' | while read -r r; do
                cls=$(registered_class "$f" "$oracle" "$r")
                echo -e "      ${DIM}[${cls:-UNREGISTERED}] $r${NC}"
            done
            checked=$((checked + 1))
            continue
        fi

        if [ -z "$new" ] && [ -z "$gone" ]; then
            printf "  %-42s %-7s ${GREEN}ok${NC} ${DIM}(pass=%s fail=%s skip=%s)${NC}\n" \
                "$f" "$oracle" "$pass" "${fails:-0}" "${skips:-0}"
            checked=$((checked + 1))
        else
            printf "  %-42s %-7s ${RED}register mismatch${NC} ${DIM}(pass=%s fail=%s skip=%s)${NC}\n" \
                "$f" "$oracle" "$pass" "${fails:-0}" "${skips:-0}"
            echo "$new" | grep -v '^$' | sed 's/^/      + unregistered: /' || true
            echo "$gone" | grep -v '^$' | sed 's/^/      - no longer differs: /' || true
            problems=$((problems + 1))
        fi
    done
done

echo
if [ "$LIST_ONLY" = 1 ]; then
    echo -e "${DIM}--list: reported $checked file/oracle pairs, checked nothing.${NC}"
    exit 0
fi
# How much of the register this run could not look at. A green line that does
# not say "and half the register went unchecked" is the same silence the
# smoke test above exists to prevent, one level up.
unchecked=0
for o in chibi gauche; do
    case " ${ORACLES[*]} " in
        *" $o "*) ;;
        *) n=$(awk -F'\t' -v o="$o" '!/^#/ && NF>=5 && $2==o' "$REPO/$REGISTER" | wc -l | tr -d ' ')
           unchecked=$((unchecked + n))
           echo -e "${YELLOW}$n register row(s) for $o went unchecked — it is not installed.${NC}" ;;
    esac
done

if [ "$problems" -eq 0 ]; then
    if [ "$unchecked" -gt 0 ]; then
        echo -e "${YELLOW}PARTIAL: the oracles that ran match the register${NC} ($checked pairs, ${ORACLES[*]}), but $unchecked row(s) were not checked at all."
        exit 0
    fi
    echo -e "${GREEN}Oracle divergences match the register${NC} ($checked file/oracle pairs, oracles: ${ORACLES[*]})"
    exit 0
fi

cat <<EOF

$(echo -e "${RED}$problems file/oracle pair(s) disagree with the register.${NC}")

A "+ unregistered" line is a difference nobody has explained yet. Classify it
in $REGISTER — and classify it honestly:
reach for oracle-defect only with evidence, not an impression. When the field
splits and the standard is silent, the class is spec-silent or
needs-investigation, and no upstream bug report follows from it.

A "- no longer differs" line means the oracle changed, or we did. Find out
which before deleting the row: if an oracle-defect stopped reproducing, that
is a fixed upstream bug worth noting; if a latitude row stopped differing, one
of us moved.
EOF
exit 1
