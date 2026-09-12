#!/usr/bin/env bash
# H3's manual lane. Dependencies: cargo, Chibi, Racket + r7rs-lib, and raco.
# All observations and minimized cases survive in the printed output directory.
set -euo pipefail
cd "$(dirname "$0")/.."
repo=$(pwd)
export H3_SEED=286
while [ "$#" -gt 0 ]; do
    case "$1" in
        --seed) export H3_SEED=${2:?missing seed}; shift 2 ;;
        --case) export H3_CASE=${2:?missing case index}; shift 2 ;;
        --historical) export H3_HISTORICAL=1; shift ;;
        --output) export H3_OUTPUT=${2:?missing output directory}; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--seed N] [--case INDEX] [--historical] [--output NEW_DIR]"
            echo "Requires Chibi and Racket with r7rs-lib. CHIBI/RACKET/RACO override executable paths."
            echo "H3_PATINA overrides the runtime binary; otherwise builds target/release/patina."
            echo "PLTUSERHOME may select an isolated Racket user package profile."
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done
if [ -z "${H3_OUTPUT:-}" ]; then
    mkdir -p target/hygiene-h3
    H3_OUTPUT=$(mktemp -d "$repo/target/hygiene-h3/sweep.XXXXXX")
else
    # Do not overwrite a previous sweep or mix its observations into this one.
    mkdir "$H3_OUTPUT"
    H3_OUTPUT=$(cd "$H3_OUTPUT" && pwd)
fi
export H3_OUTPUT
echo "H3 retained results: $H3_OUTPUT"
git rev-parse HEAD > "$H3_OUTPUT/harness-revision.txt"
git diff --stat >> "$H3_OUTPUT/harness-revision.txt"
cksum scripts/run_hygiene_differential.sh crates/patina-tests/tests/hygiene_metamorphic.rs crates/patina-tests/tests/hygiene/{generator,extended,runner,shrink,oracles,differential}.rs > "$H3_OUTPUT/harness-files.txt"
if [ -z "${H3_PATINA:-}" ]; then
    cargo build --release
    export H3_PATINA="$repo/target/release/patina"
    git rev-parse HEAD > "$H3_OUTPUT/runtime-revision.txt"
else
    echo "Override: $H3_PATINA (revision supplied by caller)" > "$H3_OUTPUT/runtime-revision.txt"
fi
cksum "$H3_PATINA" > "$H3_OUTPUT/runtime-binary.txt"
cargo test -p patina-tests --test hygiene_metamorphic differential::generated_programs_match_external_oracles -- --ignored --exact --nocapture
