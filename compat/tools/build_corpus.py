#!/usr/bin/env python3
"""Build the vendored third-party compatibility corpus in compat/vendor/.

Pipeline: fetch the snow-fort index -> rank packages by dependency in-degree ->
resolve each licence -> vendor the acceptable ones -> emit MANIFEST.json,
INVENTORY.md and REVIEW-QUEUE.json.

    python3 compat/tools/build_corpus.py            # rebuild from the pinned cache
    python3 compat/tools/build_corpus.py --refresh  # ask upstream what is new
    python3 compat/tools/build_corpus.py --check    # rebuild into a temp dir and diff

The default does not touch the network. Use it for the recurring job: Patina
bundles a library, so the corpus must drop the package that provides it. Use
--refresh only when refreshing the corpus is the intent -- it takes the highest
version per package from a freshly fetched index, so it can bump upstream
versions, and folding that into an unrelated change is how the corpus drifts
without anyone deciding to.

Requires only Python 3.9+ and curl. This is corpus construction; running and
scoring the corpus is a separate concern (see PRD/TRACK_L_SNOW_LIBRARIES_PRD.md,
work item L3).

Three traps worth knowing, all previously hit and now guarded against:
  * HTML text extraction MUST normalise whitespace after stripping tags, or a
    tag sitting inside a licence grant turns "free of charge" into
    "free of  charge" and the match silently fails.
  * Re-vendoring must not delete the hand-written docs that live in
    compat/vendor/ (README.md, LICENSES.md). See KEEP below.
  * Anything fetched must be cached, and any licence already established must
    be readable back. Ten packages are licensed only by their canonical SRFI
    document, which was once fetched every run and never kept, so a run without
    network silently deleted them from the corpus. See recorded_licences().
"""
from __future__ import annotations

import argparse, filecmp, hashlib, html, json, os, re, shutil, subprocess, sys, tarfile, tempfile
from collections import Counter
from datetime import date, timezone
from pathlib import Path

HOST = "http://snow-fort.org"
INDEX = "/s/repo.scm"
ROOT = Path(__file__).resolve().parents[2]
COMPAT = ROOT / "compat"
VENDOR = COMPAT / "vendor"
CACHE = COMPAT / ".cache"
# Hand-written files in compat/vendor/ that a rebuild must never clobber.
KEEP = {"README.md", "LICENSES.md"}

# ─────────────────────────────── s-expressions ───────────────────────────────

TOK = re.compile(r'"(?:[^"\\]|\\.)*"|[()]|[^\s()]+')


def parse_sexp(text: str):
    toks = TOK.findall(text)
    pos = 0

    def rd():
        nonlocal pos
        t = toks[pos]
        pos += 1
        if t == "(":
            out = []
            while toks[pos] != ")":
                out.append(rd())
            pos += 1
            return out
        if t.startswith('"'):
            return t[1:-1].encode().decode("unicode_escape")
        return t

    out = []
    while pos < len(toks):
        out.append(rd())
    return out


def field(node, key):
    for x in node[1:]:
        if isinstance(x, list) and x and x[0] == key:
            return x
    return None


def libname(n):
    return tuple(str(c) for c in n if isinstance(c, str))


def collect_depends(node, out):
    """Dependencies can be nested inside cond-expand clauses, so recurse."""
    if not isinstance(node, list):
        return
    if node and node[0] == "depends":
        for d in node[1:]:
            if isinstance(d, list) and d:
                out.add(libname(d))
        return
    for c in node:
        collect_depends(c, out)


# ──────────────────────────────── networking ─────────────────────────────────


def slug(name) -> str:
    """The directory/key name for a library name.

    Accepts the tuple form (`["chibi", "string"]`) or the space-joined form
    recorded in the generated files (`"chibi string"`). One conversion, because
    the two must agree: a component containing a space would otherwise be split
    by one spelling and not the other.
    """
    parts = name.split() if isinstance(name, str) else name
    return "-".join(parts)


def cache_path(url: str) -> Path:
    """Where a fetched URL lives in the cache. One scheme for everything.

    The whitelist keeps this safe for absolute URLs (colons, query strings) as
    well as the index's relative paths, and it produces exactly the names the
    tarballs already use — `/s/a/b.tgz` has no character it rewrites but the
    slashes — so adopting it does not orphan an existing cache.
    """
    CACHE.mkdir(parents=True, exist_ok=True)
    return CACHE / re.sub(r"[^A-Za-z0-9._-]", "_", url.strip("/"))


def curl(url: str, dest: Path | None = None, offline: bool = False) -> str | None:
    if offline:
        return None
    cmd = ["curl", "-sS", "-m", "60", url]
    if dest:
        cmd = ["curl", "-sS", "-m", "60", "-o", str(dest), url]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        return None
    return r.stdout if not dest else ""


def page_text(url: str, offline: bool) -> str:
    """Fetch a page as plain text, through the cache.

    Cached like the index and the tarballs. These pages used to be the one
    thing fetched every run and kept nothing of, which is what made a
    cache-only rebuild lossy rather than merely slower.

    Normalising whitespace after stripping tags is load-bearing -- see the
    module docstring.
    """
    cached = cache_path(url)
    if offline:
        raw = cached.read_text(encoding="utf-8", errors="ignore") if cached.exists() else ""
    else:
        # A refresh re-reads the page: a licence grant can be corrected upstream
        # without the package changing, and asking is the whole point of asking.
        raw = curl(url) or ""
        if raw:
            cached.write_text(raw, encoding="utf-8")
    if not raw:
        return ""
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", raw)))


# ───────────────────────────── licence resolution ────────────────────────────

SPDX = re.compile(r"SPDX-License-Identifier:\s*([A-Za-z0-9.\-+ ()]+)")
GRANT_MIT = re.compile(r"Permission is hereby granted, free of charge, to any person obtaining", re.I)
JAFFER = re.compile(r"Permission to copy this software, to modify it, to redistribute it", re.I)
MITSCHEME = re.compile(r"Permission to copy and modify this software, to\s*;*\s*redistribute", re.I)
DOCLIC = re.compile(r"this document itself may not be modified in any way", re.I)
PUBDOM = re.compile(r"(is|placed) in the public domain|public domain\.", re.I)
TEXT_RULES = [
    ("GPL-3.0", re.compile(r"GNU General Public License.{0,200}version 3", re.I | re.S)),
    ("GPL", re.compile(r"GNU (Affero )?General Public License", re.I)),
    ("LGPL", re.compile(r"GNU Lesser General Public", re.I)),
    ("Apache-2.0", re.compile(r"Apache License,?\s+Version 2\.0", re.I)),
    ("MPL-2.0", re.compile(r"Mozilla Public License", re.I)),
    ("CC0-1.0", re.compile(r"CC0 1\.0|Creative Commons Zero", re.I)),
    ("BSD", re.compile(r"Redistribution and use in source and binary forms", re.I)),
    ("MIT", GRANT_MIT),
    ("ISC", re.compile(r"Permission to use, copy, modify, and(/or)? distribute", re.I)),
]
STANDARD = {"MIT", "BSD", "ISC", "Apache-2.0", "CC0-1.0", "public-domain", "Expat",
            "BSD-3-Clause", "BSD-2-Clause", "Unlicense", "Zlib"}
COPYLEFT = re.compile(r"GPL|AGPL|LGPL|MPL|EPL|CDDL", re.I)
ALIASES = {"bsd": "BSD", "mit": "MIT", "isc": "ISC", "gpl": "GPL", "gpl3": "GPL-3.0",
           "gpl2": "GPL-2.0", "lgpl": "LGPL", "cc0": "CC0-1.0", "mpl-2.0": "MPL-2.0",
           "public-domain": "public-domain", "expat (mit": "MIT"}
TEXTUAL = (".scm", ".sld", ".sps", ".md", ".txt", ".html", ".c", ".h")


def normalise(lic: str | None) -> str | None:
    if not lic:
        return None
    s = lic.strip().strip("()").replace("_", "-")
    return ALIASES.get(s.lower(), s)


def classify(final: str | None) -> str:
    if not final:
        return "UNKNOWN"
    if final == "SRFI-document-licence":
        return "DOCUMENT-ONLY"
    if final in ("SLIB-Jaffer", "MIT-Scheme-old"):
        return "NONSTANDARD"
    if COPYLEFT.search(final):
        return "COPYLEFT"
    return "PERMISSIVE" if final in STANDARD else "NONSTANDARD"


def read_blob(d: Path) -> str:
    out = []
    for p in d.rglob("*"):
        if p.is_file() and p.suffix in TEXTUAL:
            try:
                out.append(p.read_text(encoding="utf-8", errors="ignore")[:40000])
            except OSError:
                pass
    return "\n".join(out)


def resolve_licence(pkg: dict, blob: str, offline: bool) -> tuple[str | None, str]:
    """Return (licence, evidence). Ordered most reliable first."""
    spdx = sorted({m.group(1).strip().rstrip(".").strip() for m in SPDX.finditer(blob)})
    if spdx:
        return " AND ".join(spdx), "spdx"
    if JAFFER.search(blob):
        return "SLIB-Jaffer", "licence-text"
    if MITSCHEME.search(blob):
        return "MIT-Scheme-old", "licence-text"
    if pkg.get("license"):
        return normalise(pkg["license"]), "index-field"
    for name, rx in TEXT_RULES:
        if rx.search(blob):
            return name, "text-match"
    if PUBDOM.search(blob):
        return "public-domain", "licence-text"
    # SRFI reference implementations are licensed by their defining document.
    if pkg["name"][0] == "srfi" and len(pkg["name"]) > 1:
        txt = page_text(f"https://srfi.schemers.org/srfi-{pkg['name'][1]}/srfi-{pkg['name'][1]}.html", offline)
        if txt:
            if GRANT_MIT.search(txt):
                return "MIT", "srfi-canonical-document"
            if DOCLIC.search(txt):
                # Documentation licence, not a software licence: not vendorable.
                return "SRFI-document-licence", "srfi-canonical-document"
    return None, "none"


# ──────────────────────────────── pipeline ───────────────────────────────────


def recorded_licences() -> dict[str, tuple[str, str, str]]:
    """slug -> (tarball_sha256, licence, evidence) from the generated files.

    What a cache-only rebuild answers from instead of the network, and the
    reason it can reproduce the corpus at all: some packages are licensed only
    by their canonical SRFI document, so without a recorded answer they read as
    licence-unknown and vanish from the corpus. See the module docstring.

    Consulted **only when not refreshing**. Two of `resolve_licence`'s three
    sources — the index's own `license` field, and that SRFI document — are not
    determined by the tarball, so a checksum match does not actually prove they
    are unchanged; it only proves the *package* is. Reusing them is right when
    the alternative is no answer, and wrong when the user has asked to go and
    look. `--refresh` re-derives every licence from scratch.
    """
    def harvest(path: Path, records, into: dict) -> None:
        if not path.exists():
            return
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            return
        for p in records(data):
            if p.get("tarball_sha256") and p.get("license") and p.get("license_evidence"):
                into.setdefault(slug(p["library"]),
                                (p["tarball_sha256"], p["license"], p["license_evidence"]))

    out: dict[str, tuple[str, str, str]] = {}
    harvest(VENDOR / "MANIFEST.json", lambda d: d.get("packages", []), out)
    # Excluded packages carry their licence too, so a rebuild does not have to
    # establish it a second time to decide to exclude them again.
    harvest(VENDOR / "REVIEW-QUEUE.json", lambda d: d, out)
    return out


def load_index(offline: bool) -> list[dict]:
    CACHE.mkdir(parents=True, exist_ok=True)
    idx = CACHE / "repo.scm"          # its own name, predating cache_path()
    if not offline:
        # Always re-fetch under --refresh. The index is how a new version
        # becomes visible at all, so reusing a cached copy would make the one
        # flag that exists to ask upstream what is new answer from memory.
        curl(HOST + INDEX, idx)
    if not idx.exists():
        sys.exit("error: no cached index; run with --refresh to fetch one")
    repo = parse_sexp(idx.read_text())[0]
    out = []
    for p in (x for x in repo[1:] if isinstance(x, list) and x and x[0] == "package"):
        nm, ver, url, lic = field(p, "name"), field(p, "version"), field(p, "url"), field(p, "license")
        libs = [libname(field(x, "name")[1]) for x in p[1:]
                if isinstance(x, list) and x and x[0] == "library" and field(x, "name")
                and isinstance(field(x, "name")[1], list)]
        deps: set = set()
        collect_depends(p, deps)
        desc = field(p, "description")
        out.append({
            "name": libname(nm[1]) if nm and isinstance(nm[1], list) else (libs[0] if libs else ()),
            "version": ver[1] if ver and len(ver) > 1 else None,
            "url": url[1] if url and len(url) > 1 else None,
            "license": str(lic[1]) if lic and len(lic) > 1 else None,
            "libs": libs, "deps": sorted(deps - set(libs)),
            "size": int(field(p, "size")[1]) if field(p, "size") else 0,
            "desc": desc[1] if desc and len(desc) > 1 else "",
        })
    return out


def bundled_libraries() -> set[str]:
    """Library names Patina ships itself, read from lib/ rather than hardcoded.

    These are excluded from the corpus. Patina's copy is canonical, so a
    vendored duplicate has no role: nothing tests it, and anything importing it
    resolves to the bundled version anyway.

    Keeping both was briefly useful as a drift check, but the premise expired.
    Patina's `(srfi 60)` is a rename over SRFI 151 while the vendored one is
    Jaffer's SLIB implementation -- different implementations by design, so
    comparing them measures a decision rather than a defect.
    """
    out = set()
    for sld in (ROOT / "lib").rglob("*.sld"):
        rel = sld.relative_to(ROOT / "lib").with_suffix("")
        out.add(" ".join(rel.parts))
    return out


def version_key(v: str | None):
    return tuple(int(x) if x.isdigit() else 0 for x in re.split(r"[.\-]", v or "")[:4])


def build(target: Path, offline: bool) -> dict:
    if offline:
        print("note: rebuilding from the pinned cache. A package that is neither vendored nor "
              "cached reads as licence-unknown, so REVIEW-QUEUE.json and the excluded counts "
              "in INVENTORY.md may be incomplete. Pass --refresh to re-fetch.")
    pkgs = load_index(offline)
    best: dict = {}
    for p in pkgs:                                   # keep the highest version per name
        k = " ".join(p["name"])
        if k not in best or version_key(p["version"]) > version_key(best[k]["version"]):
            best[k] = p
    pkgs = list(best.values())

    indeg: Counter = Counter()
    for p in pkgs:
        for d in p["deps"]:
            indeg[d] += 1

    prior = recorded_licences() if offline else {}
    reused = 0

    # Partition the bundled packages out before anything prices a licence. They
    # leave the corpus whatever their licence says, so resolving one is wasted
    # work, and — since a bundled package is in neither generated file and so
    # has no recorded answer — it also made the reports disagree with
    # themselves: licence-unknown from the cache, permissive after a refresh,
    # for a package excluded on neither ground. Splitting the list here means
    # no report downstream has to remember to filter them out.
    bundled = bundled_libraries()
    dropped = sorted({" ".join(p["name"]) for p in pkgs
                      if bundled & {" ".join(l) for l in p["libs"]}})
    pkgs = [p for p in pkgs if not (bundled & {" ".join(l) for l in p["libs"]})]
    if dropped:
        print(f"excluded {len(dropped)} package(s) Patina bundles: {', '.join(dropped)}")

    work = CACHE / "extracted"
    work.mkdir(parents=True, exist_ok=True)
    for p in pkgs:
        p["pop"] = max([indeg.get(tuple(l), 0) for l in p["libs"]] or [0])
        if not p["url"]:
            p["licence"], p["evidence"], p["bucket"] = None, "none", "UNKNOWN"
            continue
        tar = cache_path(p["url"])
        if not tar.exists() and not offline:
            curl(HOST + p["url"], tar)
        if not tar.exists():
            p["licence"], p["evidence"], p["bucket"] = None, "unavailable", "UNKNOWN"
            continue
        p["tar_sha256"] = hashlib.sha256(tar.read_bytes()).hexdigest()
        dest = work / " ".join(p["name"]).replace("/", "_")
        if not dest.is_dir():
            dest.mkdir(parents=True, exist_ok=True)
            try:
                with tarfile.open(tar) as t:
                    t.extractall(dest, filter="data")
            except Exception:
                pass
        p["root"] = dest
        # Same tarball as the one already vendored -> same licence. This is for
        # correctness, not speed: it also skips read_blob, but that is worth
        # about 0.2s of a 0.7s rebuild (measured over 184 packages), because
        # the packages are small. The point is that the answer cannot drift.
        seen = prior.get(slug(p["name"]))
        if seen and seen[0] == p["tar_sha256"]:
            p["licence"], p["evidence"] = seen[1], seen[2]
            reused += 1
        else:
            p["licence"], p["evidence"] = resolve_licence(p, read_blob(dest), offline)
        p["bucket"] = classify(p["licence"])

    if reused:
        print(f"reused {reused} recorded licence(s) whose tarball is unchanged")

    # SLIB is one distribution by one author; infer for the members carrying no notice.
    verified = sum(1 for p in pkgs if p["name"][0] == "slib" and p["licence"] == "SLIB-Jaffer")
    if verified:
        for p in pkgs:
            if p["name"][0] == "slib" and p["bucket"] == "UNKNOWN":
                p["licence"], p["evidence"], p["bucket"] = "SLIB-Jaffer", "slib-family-inference", "NONSTANDARD"

    sel = sorted((p for p in pkgs if p["bucket"] in ("PERMISSIVE", "NONSTANDARD")),
                 key=lambda p: (-p["pop"], " ".join(p["name"])))

    target.mkdir(parents=True, exist_ok=True)
    for entry in target.iterdir():                    # never delete the hand-written docs
        if entry.name in KEEP:
            continue
        shutil.rmtree(entry) if entry.is_dir() else entry.unlink()

    manifest = []
    for p in sel:
        pkg_slug = slug(p["name"])
        src = p["root"]
        inner = [d for d in src.iterdir() if d.is_dir()]
        if len(inner) == 1 and not any(f.suffix in (".sld", ".scm") for f in src.iterdir() if f.is_file()):
            src = inner[0]
        shutil.copytree(src, target / pkg_slug)
        files = sorted(str(f.relative_to(target / pkg_slug)) for f in (target / pkg_slug).rglob("*") if f.is_file())
        manifest.append({
            "library": " ".join(p["name"]), "slug": pkg_slug, "version": p["version"],
            "license": p["licence"], "license_class": p["bucket"], "license_evidence": p["evidence"],
            "upstream_url": HOST + p["url"], "tarball_sha256": p.get("tar_sha256"),
            "popularity_indegree": p["pop"], "provides": [" ".join(l) for l in p["libs"]],
            "depends": [" ".join(d) for d in p["deps"]], "description": p["desc"][:200],
            "files": files, "modified": False,
            "needs_ffi": any(f.endswith((".stub", ".c")) for f in files),
        })

    (target / "MANIFEST.json").write_text(json.dumps({
        "generated": date.today().isoformat(),
        "source": f"snow-fort.org {INDEX}",
        "built_by": "compat/tools/build_corpus.py",
        "purpose": "compatibility test corpus only: not a dependency of Patina, not shipped with "
                   "it, and not an endorsement of any package",
        "selection": "every snow-fort package whose licence is permissive or non-standard-"
                     "permissive, ranked by dependency in-degree; copyleft, licence-unknown and "
                     "document-licence-only packages excluded",
        "policy": "unmodified upstream extractions; provenance recorded here, not inside the trees",
        "packages": manifest,
    }, indent=2) + "\n")

    # A licence is worth recording even for a package we decline to vendor: it
    # is the same answer, it cost the same to establish, and recording it with
    # the checksum it belongs to is what lets a later rebuild reuse it instead
    # of going back to the network.
    excluded = [{"library": " ".join(p["name"]), "version": p["version"], "pop": p["pop"],
                 "license": p["licence"], "license_evidence": p["evidence"],
                 "tarball_sha256": p.get("tar_sha256"),
                 "bucket": p["bucket"], "size_kb": p["size"] // 1024,
                 "url": HOST + (p["url"] or "")}
                for p in pkgs if p["bucket"] not in ("PERMISSIVE", "NONSTANDARD")]
    excluded.sort(key=lambda x: -x["pop"])
    (target / "REVIEW-QUEUE.json").write_text(json.dumps(excluded, indent=2) + "\n")
    write_inventory(target, manifest, pkgs)
    return {"selected": manifest, "all": pkgs}


def write_inventory(target: Path, manifest: list[dict], pkgs: list[dict]) -> None:
    lic = Counter(m["license"] for m in manifest)
    lines = [
        "# Corpus inventory",
        "",
        "Generated by `compat/tools/build_corpus.py` — do not edit by hand.",
        "",
        f"**{len(manifest)} packages**, {sum(len(m['files']) for m in manifest)} files. "
        f"Licences: " + ", ".join(f"{k} {v}" for k, v in lic.most_common()) + ".",
        "",
        "`Version` is the upstream package version at the time of vendoring; the exact bytes are "
        "pinned by `tarball_sha256` in `MANIFEST.json`. `In-deg` is how many other packages in the "
        "index declare a dependency on a library this package provides.",
        "",
        "| Package | Version | Licence | In-deg | Provides |",
        "|---|---|---|---:|---|",
    ]
    for m in manifest:
        prov = ", ".join(f"`({p})`" for p in m["provides"][:3]) or "—"
        if len(m["provides"]) > 3:
            prov += f" +{len(m['provides']) - 3}"
        flag = " ⚙️" if m["needs_ffi"] else ""
        lines.append(f"| `({m['library']})`{flag} | {m['version'] or '—'} | {m['license']} | "
                     f"{m['popularity_indegree']} | {prov} |")
    ffi = sum(m["needs_ffi"] for m in manifest)
    lines += ["", f"⚙️ = ships C stubs and cannot run without an FFI ({ffi} packages); score these "
                  "out-of-scope rather than failing.", "",
              "## Excluded", "",
              "| Reason | Packages |", "|---|---|"]
    for bucket, n in Counter(p["bucket"] for p in pkgs
                             if p["bucket"] not in ("PERMISSIVE", "NONSTANDARD")).most_common():
        lines.append(f"| {bucket} | {n} |")
    lines += ["", "Full detail in `REVIEW-QUEUE.json`.", ""]
    (target / "INVENTORY.md").write_text("\n".join(lines))


def adopt_manifest_timestamp(committed: Path, rebuilt: Path) -> None:
    """Give the rebuilt manifest the committed one's `generated` date.

    The manifest records the day it was written, so `--check` compared a
    rebuild stamped today against a file stamped whenever the corpus was last
    vendored, and reported drift on every day but that one — the check could
    never stay green and so could never gate anything. What it is for is the
    *content*: every other field still differs if anything drifted.
    """
    if not (committed.is_file() and rebuilt.is_file()):
        return
    try:
        stamp = json.loads(committed.read_text())["generated"]
    except (json.JSONDecodeError, KeyError, OSError):
        return
    data = json.loads(rebuilt.read_text())
    data["generated"] = stamp
    rebuilt.write_text(json.dumps(data, indent=2) + "\n")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    # The default rebuilds from the pinned cache, and --refresh is the opt-in to
    # ask upstream what is new. That way round because the refresh is the
    # consequential operation: it takes the highest version per package from a
    # freshly fetched index, so a bare run could otherwise fold an unrelated
    # version bump into a change that meant only to drop a bundled package.
    # `--offline` is kept as the name for the default it used to request.
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--refresh", action="store_true",
                      help="re-fetch the index and re-derive every licence; may pick up new versions")
    mode.add_argument("--offline", action="store_true",
                      help="alias for the default (rebuild from the pinned cache)")
    ap.add_argument("--check", action="store_true", help="rebuild into a temp dir and diff against compat/vendor")
    args = ap.parse_args()
    offline = not args.refresh

    if args.check:
        with tempfile.TemporaryDirectory() as tmp:
            out = build(Path(tmp) / "vendor", offline)
            adopt_manifest_timestamp(VENDOR / "MANIFEST.json",
                                     Path(tmp) / "vendor" / "MANIFEST.json")
            # The two files about packages we *declined* to vendor cannot be
            # rebuilt faithfully from the cache — a package that is neither
            # vendored nor cached reads as licence-unknown, which is the note
            # printed above. Comparing them offline would fail the check for a
            # reason the tool has already announced. Under --refresh they are
            # rebuilt from the index and are compared like everything else.
            incomplete_offline = ["REVIEW-QUEUE.json", "INVENTORY.md"] if offline else []
            d = filecmp.dircmp(str(VENDOR), str(Path(tmp) / "vendor"),
                               ignore=list(KEEP) + [".DS_Store"] + incomplete_offline)
            drift = d.left_only + d.right_only + d.diff_files
            print(f"{len(out['selected'])} packages; drift: {drift or 'none'}")
            sys.exit(1 if drift else 0)

    out = build(VENDOR, offline)
    print(f"vendored {len(out['selected'])} packages into {VENDOR.relative_to(ROOT)}")
    print("buckets:", dict(Counter(p["bucket"] for p in out["all"])))


if __name__ == "__main__":
    main()
