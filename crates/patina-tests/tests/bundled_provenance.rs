//! Pin every bundled third-party library file, so an unrecorded edit fails.
//!
//! The rule (audit 2026-08-10, group E; see `lib/chibi/PROVENANCE.md`):
//! bundled library files **match upstream**. When a deviation is unavoidable,
//! the edit site carries a `;; PATINA LOCAL EDIT:` marker and the tree's
//! provenance record describes it. This test is what makes the rule stick —
//! editing a pinned file without updating the hash *and* the record turns
//! into a test failure instead of a silent fork.
//!
//! The hash is FNV-1a 64 — not tamper-proof, just drift-proof, and stable by
//! specification (unlike `DefaultHasher`), with no new dependency.
//!
//! To update after a *deliberate* change:
//!   1. Restore upstream if at all possible (the provenance records name the
//!      pinned tarballs/commit to diff against).
//!   2. If not possible, mark the edit site with `;; PATINA LOCAL EDIT:` and
//!      describe the deviation in `lib/chibi/PROVENANCE.md` or the library's
//!      `.sld` header.
//!   3. Re-run with `--nocapture`; the failure message prints the new hash.

use std::path::PathBuf;

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// (repo-relative path, FNV-1a 64 of the file bytes, recorded 2026-08-12)
const PINNED: &[(&str, u64)] = &[
    ("lib/chibi/diff.scm", 0x2050c85c4e050d74),
    ("lib/chibi/diff.sld", 0xf23c1551ba46f31b),
    ("lib/chibi/optional.scm", 0xc690d10b2fa58f49),
    ("lib/chibi/optional.sld", 0x90f9ebb211b8bc6e),
    ("lib/chibi/term/ansi.scm", 0xb611532f45ff4b36),
    ("lib/chibi/term/ansi.sld", 0xcb7a30ac04c2fb00),
    ("lib/chibi/test.scm", 0x41e9de8d4b7cc1ec),
    ("lib/chibi/test.sld", 0xf810b0f46bc155d7),
    ("lib/srfi/132.sld", 0x4edefd4109baeabb),
    ("lib/srfi/132/delndups.scm", 0xabcb04a8827d44f4),
    ("lib/srfi/132/lmsort.scm", 0xf84cd67deda00bb8),
    ("lib/srfi/132/select.scm", 0xf32fd7edae9559d8),
    ("lib/srfi/132/sort.scm", 0xf50fca73a351fa49),
    ("lib/srfi/132/sortp.scm", 0xa2f0c01190fcbc5a),
    ("lib/srfi/132/vector-util.scm", 0x5647564455a96fbf),
    ("lib/srfi/132/vhsort.scm", 0x9e2b48d547525a38),
    ("lib/srfi/132/visort.scm", 0x48d82bf021f1aebe),
    ("lib/srfi/132/vmsort.scm", 0xb8afa53199eb635a),
    ("lib/srfi/132/vqsort2.scm", 0x2fd671d4fb02ad1a),
    ("lib/srfi/132/vqsort3.scm", 0xa62eaad1e7385451),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn bundled_files_match_their_provenance_records() {
    let root = repo_root();
    let mut drifted = Vec::new();
    for (path, expected) in PINNED {
        let bytes = std::fs::read(root.join(path))
            .unwrap_or_else(|e| panic!("{path} is pinned here but unreadable: {e}"));
        let actual = fnv1a(&bytes);
        if actual != *expected {
            drifted.push(format!(
                "{path}: recorded 0x{expected:016x}, now 0x{actual:016x}"
            ));
        }
    }
    assert!(
        drifted.is_empty(),
        "bundled files changed without their provenance being updated:\n  {}\n\
         Restore upstream if possible; otherwise mark the edit with\n\
         ';; PATINA LOCAL EDIT:', record it (lib/chibi/PROVENANCE.md or the\n\
         .sld header), and update the pinned hash in this file.",
        drifted.join("\n  ")
    );
}
