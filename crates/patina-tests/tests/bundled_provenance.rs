//! Pin every third-party file claimed byte-identical to upstream, so an
//! unrecorded edit fails. The rule being enforced lives in
//! `lib/srfi/PROVENANCE.md` § The rule; the failure message below is the
//! complete update procedure.
//!
//! The scope is "files claimed byte-identical to an upstream release", not
//! "files Patina ships": `test-lib/` holds libraries supplied to the test
//! lanes with `-A` rather than bundled, and they are watched the same way.
//!
//! The adapted ports one directory over (SRFI 1, 69, 113, 128, 133, 158, …)
//! are deliberately not pinned — they are not byte-identical to anything;
//! see `lib/srfi/PROVENANCE.md` for that boundary.
//!
//! `lib/srfi/130.chibi-string.scm` is pinned despite having no upstream file to
//! compare against, for the reason `132.sld` is: the pin freezes the
//! *provenance record*, so editing 250 lines of verbatim upstream bodies -- or
//! letting them drift from the byte-identical `test-lib/chibi/string.scm` the
//! records call a deliberate duplication -- becomes a deliberate act rather
//! than a silent one. `130.scm` beside it is unchanged by that inlining.
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
    // SRFI 101, byte-identical to chibi's R7RS adaptation of the SRFI's own
    // reference implementation — the only R7RS rendering of it that exists.
    // Laid out flat so its `(include "101.scm")` resolves unchanged.
    ("lib/srfi/101.sld", 0x3f16ae5f678165dd),
    ("lib/srfi/101.scm", 0xd53cf2d050279330),
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
    // SRFI 64, byte-identical to the snow-fort 0.2.1 snowball the corpus
    // vendored (`compat/vendor/srfi-64`, MIT, Per Bothner). Bundled by the
    // testing-API clause of the bundling policy, added 2026-09-06.
    ("lib/srfi/64.sld", 0x26141d34dea29f09),
    ("lib/srfi/64.scm", 0x02cc208e43562e3c),
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
    // Not byte-identical to anything: the subset of `(chibi string)` that
    // `(srfi 130)` needs, with `%` renames and a header (#198). Pinned anyway,
    // as `132.sld` is -- see the module docs. Re-pinned for #204's bounded
    // predicate/witness fixes, recorded in lib/srfi/PROVENANCE.md.
    ("lib/srfi/130.chibi-string.scm", 0xd590c6032851a1a3),
    // Re-pinned 2026-09-06 (#198): the import of `(chibi string)` became an
    // `(include "130.chibi-string.scm")` of the inlined subset, plus `(srfi 14)`
    // directly. The header moved with it. `130.scm` below is untouched by that
    // work — its hash is the one it has had all along, which is the evidence
    // the inlining changed the library's *dependencies* and not its code.
    ("lib/srfi/130.sld", 0x2377d56b49134388),
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
    // Supplied by `-A test-lib`, not bundled (see test-lib/README.md) — pinned
    // all the same, because moving a file off the shipped path does not make
    // upstream drift less worth catching. Hashes below are unchanged from when
    // these lived in `lib/chibi/`: the files moved byte-for-byte.
    ("test-lib/chibi/diff.scm", 0x2050c85c4e050d74),
    ("test-lib/chibi/diff.sld", 0xf23c1551ba46f31b),
    ("test-lib/chibi/optional.scm", 0xc690d10b2fa58f49),
    ("test-lib/chibi/optional.sld", 0x90f9ebb211b8bc6e),
    ("test-lib/chibi/string.scm", 0x40519db9f7f6ea77),
    ("test-lib/chibi/string.sld", 0x547187363ef72f66),
    ("test-lib/chibi/term/ansi.scm", 0xb611532f45ff4b36),
    ("test-lib/chibi/term/ansi.sld", 0xcb7a30ac04c2fb00),
    ("test-lib/chibi/test.scm", 0x41e9de8d4b7cc1ec),
    ("test-lib/chibi/test.sld", 0xf810b0f46bc155d7),
    // Pinned post-edit: upstream apart from the `(patina …)` cond-expand
    // branch recorded in test-lib/chibi/PROVENANCE.md. Same reason as
    // lib/srfi/130.scm — a recorded deviation must not be why the rest of a
    // file goes unwatched.
    ("test-lib/chibi/filesystem.sld", 0x030d79584ffe6de0),
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
    "lib/srfi/116",
    "lib/srfi/117",
    "lib/srfi/125",
    "lib/srfi/127",
    "lib/srfi/132",
    "lib/srfi/134",
    "lib/srfi/135",
    "lib/srfi/144",
    "test-lib/chibi",
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
         provenance home (lib/srfi/PROVENANCE.md,\n\
         test-lib/chibi/PROVENANCE.md, or\n\
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
