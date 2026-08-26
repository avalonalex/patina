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
    ("lib/chibi/filesystem.sld", 0x6081884933750547),
    ("lib/chibi/optional.scm", 0xc690d10b2fa58f49),
    ("lib/chibi/optional.sld", 0x90f9ebb211b8bc6e),
    ("lib/chibi/string.scm", 0x40519db9f7f6ea77),
    ("lib/chibi/string.sld", 0x547187363ef72f66),
    ("lib/chibi/term/ansi.scm", 0xb611532f45ff4b36),
    ("lib/chibi/term/ansi.sld", 0xcb7a30ac04c2fb00),
    ("lib/chibi/test.scm", 0x41e9de8d4b7cc1ec),
    ("lib/chibi/test.sld", 0xf810b0f46bc155d7),
    // SRFI 162's own sample implementation, byte-identical. The rest of
    // lib/srfi/128/ is the adapted SRFI 128 port and is deliberately unpinned
    // (see the module docs); this file is not adapted, so it is watched.
    ("lib/srfi/128/162-impl.scm", 0xf93547f60a36817a),
    // SRFI 116's own reference implementation (John Cowan, MIT). The impl
    // file is pinned *post-edit* — three PATINA LOCAL EDITs, marked in place
    // and recorded in PROVENANCE.md — and the `.sld` is ours, as for 117 and
    // 127 below.
    ("lib/srfi/116.sld", 0xf2edca63f4ae1e92),
    ("lib/srfi/116/ilists-base.scm", 0xde2e997658b2dc5b),
    ("lib/srfi/116/ilists-impl.scm", 0x745933629d7b4e17),
    // SRFI 117's and 127's own reference implementations (John Cowan, MIT).
    // 117's is pinned post-edit — one PATINA LOCAL EDIT to list-queue-join!,
    // marked in place and recorded in PROVENANCE.md. The two `.sld` files are
    // ours, not upstream's (upstream names the libraries `(srfi-117)` and
    // `(lseqs)`), and are pinned so an edit to them is deliberate too.
    ("lib/srfi/117.sld", 0xcbbd0e9aaac6aef6),
    ("lib/srfi/117/list-queues-impl.scm", 0x30690c9b26f3e72b),
    ("lib/srfi/127.sld", 0x6171c0c4565c6a0f),
    ("lib/srfi/127/lseqs-impl.scm", 0x13a69d50373b02fe),
    // SRFI 134's implementation is the body of upstream's own `srfi/134.sld`,
    // which ships it inline rather than as an include; the split into a
    // `.sld` and an impl file is ours, the code between them is not. Named
    // `ideque-stream-impl.scm` and not `ideque-impl.scm`, which is what every
    // sibling's naming would suggest, because upstream *has* an
    // `ideque-2list/ideque-impl.scm` and it is a different implementation —
    // the name that reads as conventional here would send a diff at the wrong
    // file.
    ("lib/srfi/134.sld", 0x8cc0b0c42ba84809),
    ("lib/srfi/134/ideque-stream-impl.scm", 0xeadd47dfb4dc28e8),
    // SRFI 144's own `srfi/144.sld` and its body files. The `.sld` carries a
    // marked local edit for include paths only; `144.r6rs.scm` carries one for
    // the infinities, which the R7RS `numerator` it delegates to does not
    // accept. Both are described in PROVENANCE.md.
    ("lib/srfi/144.sld", 0x6b65aaa445fe327e),
    ("lib/srfi/144/144.body.scm", 0x21bebf04ab1f01d2),
    ("lib/srfi/144/144.body0.scm", 0xfe113242007e0f8c),
    ("lib/srfi/144/144.constants.scm", 0x416e0cfcce318bdc),
    ("lib/srfi/144/144.r6rs.scm", 0xa7e1f30e6b5cac90),
    ("lib/srfi/144/144.special.scm", 0x447d7e808fd68d38),
    // SRFI 135, laid out exactly as upstream ships it — `135.sld` beside
    // `135.body.scm`, the kernel under `135/` — so every `include` resolves
    // unchanged and the `.sld` and kernel are upstream's byte for byte.
    // `135.body.scm` carries four marked local edits; see PROVENANCE.md.
    ("lib/srfi/135.sld", 0x522c8b29cb4b1e59),
    ("lib/srfi/135.body.scm", 0xf04196a01d1a2cf7),
    ("lib/srfi/135/kernel8.sld", 0xb2238678735d7314),
    ("lib/srfi/135/kernel8.body.scm", 0x9f9606932d824c8d),
    // SRFI 101 is the only bundle that was *ported* rather than copied:
    // upstream ships R6RS `.sls` libraries. The marked edits are the two record
    // definitions and one bitwise name; see PROVENANCE.md.
    ("lib/srfi/101.sld", 0xce37903462adb9d2),
    ("lib/srfi/101/rlist-impl.scm", 0xd3e5fefd8f9ca9c2),
    // SRFI 41's reference implementation as Retropikzel ported it,
    // byte-identical. Its `.sld` is pinned post-edit: two lines export and
    // include `stream-match`, which the port comments out because the
    // reference writes it in `syntax-case` — see lib/srfi/41-match.scm and
    // PROVENANCE.md. `41-match.scm` is chibi's, pinned post-edit for the same
    // reason as 130.scm below: a recorded deviation must not be why a file
    // goes unwatched.
    ("lib/srfi/41-match.scm", 0xd3c7746a264796c7),
    ("lib/srfi/41.scm", 0xe40e5826e7cc7130),
    ("lib/srfi/41.sld", 0x8268bc8aba0fba5a),
    // Pinned post-edit: one PATINA LOCAL EDIT (ucs-range->char-set's base
    // set), recorded in lib/srfi/PROVENANCE.md.
    ("lib/srfi/14.scm", 0xb971a3e4e5280a08),
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
    ("lib/srfi/132.sld", 0xdecd1cc07c13a2d7),
    ("lib/srfi/132/delndups.scm", 0xabcb04a8827d44f4),
    ("lib/srfi/132/lmsort.scm", 0xf84cd67deda00bb8),
    ("lib/srfi/132/select.scm", 0x9c14f2f4637715a2),
    // Pinned post-edit: one PATINA LOCAL EDIT (list-sort is the stable
    // merge sort, not upstream's tie-reversing heap sort), recorded in
    // 132.sld's header.
    ("lib/srfi/132/sort.scm", 0x5f0e8259f6ea93dd),
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
/// Directories where *every* Scheme file is vendored, so that a file added to
/// one is caught rather than silently unguarded.
///
/// Listed rather than derived from the directory component of each `PINNED`
/// path, because two of those directories are mixed and would fail: `lib/srfi`
/// holds Patina-authored `.sld` wrappers beside the vendored subdirectories,
/// and `lib/srfi/128` holds `128.body1.scm` and `128.body2.scm`, which are
/// upstream (John Cowan, MIT) but have never been pinned. That second one is a
/// real gap in this guard and is left as one deliberately: pinning those files
/// means first establishing what they are byte-identical to, which is not this
/// list's job to assume.
const PINNED_TREES: &[&str] = &[
    "lib/chibi",
    "lib/srfi/101",
    "lib/srfi/116",
    "lib/srfi/117",
    "lib/srfi/125",
    "lib/srfi/127",
    "lib/srfi/132",
    "lib/srfi/134",
    "lib/srfi/135",
    "lib/srfi/144",
];

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
