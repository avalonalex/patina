#!/usr/bin/env bash
# Test a real host outside the workspace: dev-dependency feature unification
# must not accidentally provide a backend missing from its requested features.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root=$(pwd)
consumer_dir=$(mktemp -d "${TMPDIR:-/tmp}/patina-embedding.XXXXXX")
trap 'rm -rf "$consumer_dir"' EXIT
# Separate artifacts from workspace builds; never overwrite the CLI test binary.
export CARGO_TARGET_DIR="$repo_root/target/embedding-features"
export PATINA_ISOLATED_LIBRARIES=1
mkdir -p "$consumer_dir/src"
cp crates/patina-interpreter/tests/support/feature_consumer.rs "$consumer_dir/src/main.rs"
cp Cargo.lock "$consumer_dir/Cargo.lock"
python3 - "$repo_root" "$consumer_dir" <<'PY'
import json
from pathlib import Path
import sys
root, consumer = map(Path, sys.argv[1:])
(consumer / 'Cargo.toml').write_text('''[package]
name = "patina-embedding-consumer"
version = "0.0.0"
edition = "2024"
publish = false

[features]
vm = ["patina-interpreter/vm"]
tree-walker = ["patina-interpreter/tree-walker"]
legacy-pipeline = ["patina-interpreter/legacy-pipeline", "dep:patina-pipeline"]
defaults = ["patina-interpreter/default"]

[dependencies]
patina-interpreter = { path = ''' + json.dumps(str(root / 'crates/patina-interpreter')) + ''', default-features = false }
patina-pipeline = { path = ''' + json.dumps(str(root / 'crates/patina-pipeline')) + ''', optional = true }

[workspace]
''')
PY
for features in '' vm tree-walker vm,tree-walker legacy-pipeline defaults; do
    echo "Embedding features: ${features:-none (host backend)}"
    args=(--manifest-path "$consumer_dir/Cargo.toml" --offline)
    if [[ -n "$features" ]]; then args+=(--features "$features"); fi
    cargo run "${args[@]}"
    cargo tree "${args[@]}" --edges normal --prefix none > "$consumer_dir/tree.txt"
    python3 - "$features" "$consumer_dir/tree.txt" <<'PY'
from pathlib import Path
import sys
features = sys.argv[1].split(',')
names = {line.split()[0] for line in Path(sys.argv[2]).read_text().splitlines()}
expected = {
    'patina-vm': 'vm' in features or 'defaults' in features,
    'patina-tree-walker': bool({'tree-walker', 'legacy-pipeline', 'defaults'} & set(features)),
    'patina-pipeline': 'legacy-pipeline' in features,
}
for name, present in expected.items():
    assert (name in names) == present, f'{features}: unexpected dependency presence: {name}'
print('dependency graph passed')
PY
done
