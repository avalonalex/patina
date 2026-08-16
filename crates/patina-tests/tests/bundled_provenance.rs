//! Pin every bundled file claimed byte-identical to upstream, so an
//! unrecorded edit fails. The rule being enforced lives in
//! `lib/chibi/PROVENANCE.md` § The rule; the failure message below is the
//! complete update procedure.
//!
//! The adapted ports one directory over (SRFI 1, 69, 113, 128, 133, 158, …)
//! are deliberately not pinned — they are not byte-identical to anything;
//! see `lib/srfi/PROVENANCE.md` for that boundary.
//!
//! The hash is FNV-1a 64 — not tamper-proof, just drift-proof, and stable by
//! specification (unlike `DefaultHasher`), with no new dependency.

mod common;
use common::{files_under, repo_root};
use std::collections::BTreeSet;
use std::path::Path;

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
    // Pinned post-edit: upstream apart from the `(patina …)` cond-expand branch
    // recorded in lib/chibi/PROVENANCE.md. Same reason as lib/srfi/130.scm —
    // a recorded deviation must not be why the rest of a file goes unwatched.
    ("lib/chibi/filesystem.sld", 0xe5b3fc5704428498),
    ("lib/chibi/optional.scm", 0xc690d10b2fa58f49),
    ("lib/chibi/optional.sld", 0x90f9ebb211b8bc6e),
    ("lib/chibi/string.scm", 0x40519db9f7f6ea77),
    ("lib/chibi/string.sld", 0x547187363ef72f66),
    ("lib/chibi/term/ansi.scm", 0xb611532f45ff4b36),
    ("lib/chibi/term/ansi.sld", 0xcb7a30ac04c2fb00),
    ("lib/chibi/test.scm", 0x41e9de8d4b7cc1ec),
    ("lib/chibi/test.sld", 0xf810b0f46bc155d7),
    ("lib/srfi/14.scm", 0xd1a60e0a57f95349),
    ("lib/srfi/14.sld", 0xa30fdc16bb8de140),
    ("lib/srfi/27.scm", 0xf12c3dd28221b826),
    ("lib/srfi/27.sld", 0xa55b16b061696cdf),
    // 130.scm is pinned at its *post-edit* hash, like 132.sld below: it is
    // upstream apart from one `;; PATINA LOCAL EDIT:`, and pinning the result
    // is what keeps the other 307 lines guarded. Leaving a file out because it
    // is not byte-identical would make the deviation the reason its whole tree
    // goes unwatched.
    ("lib/srfi/130.scm", 0x2979bbeb162b21e1),
    // Re-pinned 2026-08-15: its Patina-authored header now cites the in-repo
    // BSD text instead of a bare URL. Header only; the library form below it
    // is untouched upstream.
    ("lib/srfi/130.sld", 0x1fe6cb5ef2698643),
    // Unlike every other row, 132.sld is Patina-authored with no upstream to
    // match — the pin freezes the tree's provenance *record*, so editing the
    // header is a deliberate act like editing the files it describes.
    // Byte-identical to chibi 0.12.0's, per lib/srfi/PROVENANCE.md. Its `.sld`
    // is Patina's own and so is not pinned.
    ("lib/srfi/125/hash.scm", 0xf1aeae0530c9f659),
    ("lib/srfi/132.sld", 0xe954f0fc6682a300),
    ("lib/srfi/132/delndups.scm", 0xabcb04a8827d44f4),
    ("lib/srfi/132/lmsort.scm", 0xf84cd67deda00bb8),
    ("lib/srfi/132/select.scm", 0x9c14f2f4637715a2),
    ("lib/srfi/132/sort.scm", 0xf50fca73a351fa49),
    ("lib/srfi/132/sortp.scm", 0xa2f0c01190fcbc5a),
    ("lib/srfi/132/vector-util.scm", 0x5647564455a96fbf),
    ("lib/srfi/132/vhsort.scm", 0x9e2b48d547525a38),
    ("lib/srfi/132/visort.scm", 0x48d82bf021f1aebe),
    ("lib/srfi/132/vmsort.scm", 0xb8afa53199eb635a),
    ("lib/srfi/132/vqsort2.scm", 0x2fd671d4fb02ad1a),
    ("lib/srfi/132/vqsort3.scm", 0xa62eaad1e7385451),
];

/// The trees whose `.scm`/`.sld` files must ALL appear in [`PINNED`]. The
/// provenance records say "every file in this tree"; without this, a file
/// *added* to a pinned tree would be unguarded while the records stay green.
const PINNED_TREES: &[&str] = &["lib/chibi", "lib/srfi/132"];

fn scheme_files_under(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for path in files_under(dir) {
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("scm") | Some("sld")
        ) {
            let rel = path.strip_prefix(root).expect("under repo root");
            out.insert(rel.to_string_lossy().into_owned());
        }
    }
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
         Restore upstream if possible (the provenance records name the pinned\n\
         tarballs/commit to diff against). Otherwise: mark the edit site with\n\
         ';; PATINA LOCAL EDIT:', describe the deviation in the tree's\n\
         provenance home (lib/chibi/PROVENANCE.md, lib/srfi/PROVENANCE.md, or\n\
         the library's .sld header), and update the pinned hash above.",
        drifted.join("\n  ")
    );
}

#[test]
fn pinned_trees_have_no_unpinned_files() {
    let root = repo_root();
    let mut on_disk = BTreeSet::new();
    for tree in PINNED_TREES {
        scheme_files_under(&root, &root.join(tree), &mut on_disk);
    }
    // Prefix with the separator so "lib/srfi/132.sld" (a Patina-authored
    // sibling file) does not count as inside the "lib/srfi/132" tree.
    let pinned: BTreeSet<String> = PINNED
        .iter()
        .map(|(p, _)| p.to_string())
        .filter(|p| PINNED_TREES.iter().any(|t| p.starts_with(&format!("{t}/"))))
        .collect();
    assert_eq!(
        on_disk, pinned,
        "the pinned trees and the PINNED table disagree — a file was added to\n\
         (or removed from) a byte-identical tree without updating this guard\n\
         and the tree's provenance record"
    );
}
