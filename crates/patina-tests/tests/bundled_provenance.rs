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
    // `lib/srfi/14.*` was pinned here until #372. It is no longer third-party:
    // Olin Shivers' reference implementation is Latin-1 by construction — a
    // char-set is a 256-character string indexed by code point — so making a
    // char-set hold any character meant replacing it rather than editing it.
    // The Patina-authored implementation is out of this list's scope, like the
    // other adapted ports one directory over; lib/srfi/PROVENANCE.md records
    // what it replaced and why.
    //
    // SRFI 159, byte-identical to the tarball recorded in
    // lib/srfi/PROVENANCE.md. Ten of its files carry no licence notice — see
    // that record for how the licence was established rather than inferred,
    // which is the reason this one is bundled where (srfi 4) was not.
    ("lib/srfi/159.sld", 0xa407641c8392965d),
    ("lib/srfi/159/base.sld", 0xc40c95006bfcdd38),
    ("lib/srfi/159/color.scm", 0x2d416b074fba45c1),
    ("lib/srfi/159/color.sld", 0xde7958442e34010e),
    ("lib/srfi/159/column.scm", 0x8aff7d4b1ea624af),
    ("lib/srfi/159/columnar.sld", 0x6b6f5909f72cb73d),
    ("lib/srfi/159/internal/base.scm", 0x6b5adfcf3c39c12c),
    ("lib/srfi/159/internal/base.sld", 0x0038f928b33d28ce),
    ("lib/srfi/159/internal/compat.sld", 0x10b3a8832d418d2b),
    ("lib/srfi/159/internal/monad.scm", 0xb1a9aef4f5eac55b),
    ("lib/srfi/159/internal/pretty.scm", 0xd8289ecce8696a96),
    ("lib/srfi/159/internal/pretty.sld", 0x588585eb814777bc),
    ("lib/srfi/159/internal/util.scm", 0x918ae187a947aa83),
    ("lib/srfi/159/internal/util.sld", 0x44666082df285bdf),
    ("lib/srfi/159/internal/write.scm", 0xb875b9d52bacc6cb),
    ("lib/srfi/159/unicode.scm", 0xda5fa29d72ac51dd),
    ("lib/srfi/159/unicode.sld", 0x505181e5fcf138d6),
    // SRFI 231, chibi's own implementation, pinned *post-edit*: two
    // substitutions marked by four PATINA LOCAL EDITs replace its two
    // (chibi assert) imports and source u1vector from (patina bitvector).
    // Pinning the result is what keeps the other ~1,400 lines guarded, as for
    // 130.scm and 117's impl; lib/srfi/231 is in PINNED_TREES below, which is
    // what notices a fifth file arriving.
    ("lib/srfi/231.sld", 0xbd4d43f814e8e83b),
    ("lib/srfi/231/base.scm", 0xbb236265def20906),
    ("lib/srfi/231/base.sld", 0x38e27820c5d4ec8e),
    ("lib/srfi/231/transforms.scm", 0xe80b71d315de15e3),
    // SRFI 165, byte-identical to the tarball recorded in
    // lib/srfi/PROVENANCE.md. Both files carry the full MIT text inline, so
    // nothing about its licence had to be established.
    ("lib/srfi/165.sld", 0x68089aac5cdd7be9),
    ("lib/srfi/165.scm", 0x18c17bf9dedb0cd3),
    // SRFI 115, byte-identical to the tarball recorded in
    // lib/srfi/PROVENANCE.md. Unlike its neighbours here — SRFI 159 above,
    // whose licence took three checks to establish, and SRFI 160 below, whose
    // (srfi 4) layer had none at all — nothing about its licence had to be
    // established: every file carries an explicit SPDX identifier.
    ("lib/srfi/115.sld", 0x57df407358c44250),
    ("lib/srfi/115.scm", 0xf0378e3a7f03501f),
    ("lib/srfi/115/boundary.sld", 0xaeba3a21ab466dd8),
    ("lib/srfi/115/boundary.scm", 0xa5f518ae15abaf21),
    // SRFI 160, from the tarball recorded in lib/srfi/PROVENANCE.md.
    // `(srfi 4)` is deliberately absent: it is Patina-authored, not
    // third-party, because the SRFI's own contrib port carries no licence
    // notice on any of its files — see that file's header. SRFI 160's
    // per-type files are its own atexpander.sh output rather than tarball
    // content — the one generated tree in lib/ — so pinning them is what
    // makes an edit to one deliberate rather than indistinguishable from a
    // re-expansion.
    ("lib/srfi/160/base.sld", 0xf8374cb34d7140b3),
    ("lib/srfi/160/base/c128-vector2list.scm", 0x1e32dda0a66cc5dd),
    ("lib/srfi/160/base/c64-vector2list.scm", 0x8e4a73d497c7a45f),
    ("lib/srfi/160/base/complex.scm", 0xb0916bd113c6aced),
    ("lib/srfi/160/base/f32-vector2list.scm", 0x871606020db60c57),
    ("lib/srfi/160/base/f64-vector2list.scm", 0xca9b7fc7f3795261),
    ("lib/srfi/160/base/r7rec.scm", 0x43f4e01b94466589),
    ("lib/srfi/160/base/s16-vector2list.scm", 0x55618baa20760879),
    ("lib/srfi/160/base/s32-vector2list.scm", 0x4d348a7a082162f9),
    ("lib/srfi/160/base/s64-vector2list.scm", 0x2ff4100705b2eb5f),
    ("lib/srfi/160/base/s8-vector2list.scm", 0xd8f40ab46e0a4855),
    ("lib/srfi/160/base/u16-vector2list.scm", 0x0011352deff68811),
    ("lib/srfi/160/base/u32-vector2list.scm", 0x48b98e51772a75a9),
    ("lib/srfi/160/base/u64-vector2list.scm", 0xdeb59b1b9aa19e8f),
    ("lib/srfi/160/base/u8-vector2list.scm", 0x154f7f645b84146d),
    ("lib/srfi/160/base/valid.scm", 0xa8af6691189e2679),
    ("lib/srfi/160/c128-impl.scm", 0xcdb6f1657c75bd91),
    ("lib/srfi/160/c128.sld", 0xb366d2e0e47048a5),
    ("lib/srfi/160/c64-impl.scm", 0x6a6a0dc399de19da),
    ("lib/srfi/160/c64.sld", 0xe130ef5c2376b32b),
    ("lib/srfi/160/f32-impl.scm", 0xd7dc9fad3c9853e2),
    ("lib/srfi/160/f32.sld", 0x2f784b291e6ce963),
    ("lib/srfi/160/f64-impl.scm", 0x5421368c8a7c489b),
    ("lib/srfi/160/f64.sld", 0x417bd6fdcf1719bb),
    ("lib/srfi/160/s16-impl.scm", 0xb0fdca8ab0b8074d),
    ("lib/srfi/160/s16.sld", 0x50239b383a538d03),
    ("lib/srfi/160/s32-impl.scm", 0x25532fac70be0473),
    ("lib/srfi/160/s32.sld", 0xaa756488e4c7eaeb),
    ("lib/srfi/160/s64-impl.scm", 0x4ecf0636f94f96ea),
    ("lib/srfi/160/s64.sld", 0xf5afa7711af2226b),
    ("lib/srfi/160/s8-impl.scm", 0x0ef0673ffb615ca4),
    ("lib/srfi/160/s8.sld", 0xbfb80ce5c382c081),
    ("lib/srfi/160/u16-impl.scm", 0x793efa933d2c395b),
    ("lib/srfi/160/u16.sld", 0xda1c86e8766ff507),
    ("lib/srfi/160/u32-impl.scm", 0x14d703ad5517d705),
    ("lib/srfi/160/u32.sld", 0xeed897eb0dd77df7),
    ("lib/srfi/160/u64-impl.scm", 0x216f048fdf07cf2c),
    ("lib/srfi/160/u64.sld", 0xb457b197757e74cf),
    ("lib/srfi/160/u8-impl.scm", 0xfa6b5e350ce2676e),
    ("lib/srfi/160/u8.sld", 0x0f3a12ffa6b8d63d),
    // SRFI 146, byte-identical to the tarball recorded in
    // lib/srfi/PROVENANCE.md, together with the supporting libraries it ships
    // under their authors' namespaces. Those two trees are bundled verbatim
    // rather than renamed, which is what makes pinning them meaningful — see
    // that file's "SRFI 146 is byte-identical" section for why.
    ("lib/srfi/146.sld", 0x8dca2df9ac8a48ed),
    ("lib/srfi/146.scm", 0xcc5511f53cb6a593),
    ("lib/srfi/146/hash.sld", 0x180632406d7db918),
    ("lib/srfi/146/hash.scm", 0x7001bc958ed65227),
    ("lib/nieper/rbtree.sld", 0x4592f90b650c5f68),
    ("lib/nieper/rbtree.scm", 0x52e234d5e8937cd9),
    ("lib/gleckler/hamt.sld", 0xbe7042d760c5ba50),
    ("lib/gleckler/hamt.scm", 0x3411ac21a79786e9),
    ("lib/gleckler/hamt-map.sld", 0x01e5f57c38a482f2),
    ("lib/gleckler/hamt-map.scm", 0x13f965bd90370d93),
    ("lib/gleckler/hamt-misc.sld", 0xf799bd0755a5b164),
    ("lib/gleckler/hamt-misc.scm", 0xec9f93e907a79960),
    ("lib/gleckler/vector-edit.sld", 0x9c1a797340415414),
    ("lib/gleckler/vector-edit.scm", 0x25cdcce938f81ab0),
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
    // SRFI 159's two directories. Without both named here the hash list is
    // the only guard on this tree, and it cannot see an *added* file: a
    // seventeenth file dropped under `159/internal` would be unpinned and
    // unnoticed, which is the hole this guard exists to close.
    // Added with SRFI 159, and `lib/srfi/115` with it: #388 pinned that tree's
    // files without listing the tree, so an added file went unguarded there
    // too. Every Scheme file in both is vendored, which is what qualifies
    // them.
    "lib/srfi/115",
    "lib/srfi/159",
    "lib/srfi/159/internal",
    // SRFI 160's two directories. This is the strongest case in the list for
    // the tree guard rather than the hash list alone: every file in them is
    // `atexpander.sh` output, so a *thirteenth* type dropped in by a re-run of
    // a modified expander is exactly the addition a per-file hash cannot see
    // and this guard can.
    "lib/srfi/160",
    "lib/srfi/160/base",
    // SRFI 231's directory, for the same reason and with the same
    // qualification: every Scheme file under it is vendored from chibi. The
    // hash list above pins four files *post-edit*, which cannot see a fifth
    // arriving — a refresh from a newer chibi is exactly how one would.
    "lib/srfi/231",
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
