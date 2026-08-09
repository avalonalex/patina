#!/usr/bin/env python3
"""Build the vendored third-party compatibility corpus in compat/vendor/.

Pipeline: fetch the snow-fort index -> rank packages by dependency in-degree ->
resolve each licence -> vendor the acceptable ones -> emit MANIFEST.json,
INVENTORY.md and REVIEW-QUEUE.json.

    python3 compat/tools/build_corpus.py            # full run (uses the cache)
    python3 compat/tools/build_corpus.py --offline  # cache only, no network
    python3 compat/tools/build_corpus.py --check    # rebuild into a temp dir and diff

Requires only Python 3.9+ and curl. This is corpus construction; running and
scoring the corpus is a separate concern (see PRD/TRACK_L_SNOW_LIBRARIES_PRD.md,
work item L3).

Two traps worth knowing, both previously hit and now guarded against:
  * HTML text extraction MUST normalise whitespace after stripping tags, or a
    tag sitting inside a licence grant turns "free of charge" into
    "free of  charge" and the match silently fails.
  * Re-vendoring must not delete the hand-written docs that live in
    compat/vendor/ (README.md, LICENSES.md). See KEEP below.
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
    """Fetch a page as plain text. Normalising whitespace here is load-bearing."""
    raw = curl(url, offline=offline)
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


def load_index(offline: bool) -> list[dict]:
    CACHE.mkdir(parents=True, exist_ok=True)
    idx = CACHE / "repo.scm"
    if not idx.exists():
        if offline:
            sys.exit("error: no cached index and --offline given")
        curl(HOST + INDEX, idx)
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

    Used only to flag vendored packages that duplicate a bundled library, so the
    drift guard in crates/patina-tests/tests/bundled_vs_vendored.rs can check
    the port against its upstream reference.

    TODO(L4): this is interim. Once each bundled port is a faithful import of
    upstream, the bundled version becomes canonical and packages in this set
    should be *excluded* from the corpus rather than flagged -- change the
    `sel` filter below to drop them, then delete this function's flag, the
    drift guard, and the `bundled_by_patina` field. See
    PRD/TRACK_L_SNOW_LIBRARIES_PRD.md section L4.
    """
    out = set()
    for sld in (ROOT / "lib").rglob("*.sld"):
        rel = sld.relative_to(ROOT / "lib").with_suffix("")
        out.add(" ".join(rel.parts))
    return out


def version_key(v: str | None):
    return tuple(int(x) if x.isdigit() else 0 for x in re.split(r"[.\-]", v or "")[:4])


def build(target: Path, offline: bool) -> dict:
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

    work = CACHE / "extracted"
    work.mkdir(parents=True, exist_ok=True)
    for p in pkgs:
        p["pop"] = max([indeg.get(tuple(l), 0) for l in p["libs"]] or [0])
        if not p["url"]:
            p["licence"], p["evidence"], p["bucket"] = None, "none", "UNKNOWN"
            continue
        tar = CACHE / p["url"].strip("/").replace("/", "_")
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
        p["licence"], p["evidence"] = resolve_licence(p, read_blob(dest), offline)
        p["bucket"] = classify(p["licence"])

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
        slug = "-".join(p["name"])
        src = p["root"]
        inner = [d for d in src.iterdir() if d.is_dir()]
        if len(inner) == 1 and not any(f.suffix in (".sld", ".scm") for f in src.iterdir() if f.is_file()):
            src = inner[0]
        shutil.copytree(src, target / slug)
        files = sorted(str(f.relative_to(target / slug)) for f in (target / slug).rglob("*") if f.is_file())
        manifest.append({
            "library": " ".join(p["name"]), "slug": slug, "version": p["version"],
            "license": p["licence"], "license_class": p["bucket"], "license_evidence": p["evidence"],
            "upstream_url": HOST + p["url"], "tarball_sha256": p.get("tar_sha256"),
            "popularity_indegree": p["pop"], "provides": [" ".join(l) for l in p["libs"]],
            "depends": [" ".join(d) for d in p["deps"]], "description": p["desc"][:200],
            "files": files, "modified": False,
            "needs_ffi": any(f.endswith((".stub", ".c")) for f in files),
            # Patina ships its own port of this library. Such packages are not
            # merely corpus subjects: the vendored copy is the canonical
            # upstream reference that the bundled port is checked against.
            "bundled_by_patina": bundled_libraries() & {" ".join(l) for l in p["libs"]} != set(),
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

    excluded = [{"library": " ".join(p["name"]), "version": p["version"], "pop": p["pop"],
                 "license": p["licence"], "bucket": p["bucket"], "size_kb": p["size"] // 1024,
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


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--offline", action="store_true", help="use only the local cache")
    ap.add_argument("--check", action="store_true", help="rebuild into a temp dir and diff against compat/vendor")
    args = ap.parse_args()

    if args.check:
        with tempfile.TemporaryDirectory() as tmp:
            out = build(Path(tmp) / "vendor", args.offline)
            d = filecmp.dircmp(str(VENDOR), str(Path(tmp) / "vendor"),
                               ignore=list(KEEP) + [".DS_Store"])
            drift = d.left_only + d.right_only + d.diff_files
            print(f"{len(out['selected'])} packages; drift: {drift or 'none'}")
            sys.exit(1 if drift else 0)

    out = build(VENDOR, args.offline)
    print(f"vendored {len(out['selected'])} packages into {VENDOR.relative_to(ROOT)}")
    print("buckets:", dict(Counter(p["bucket"] for p in out["all"])))


if __name__ == "__main__":
    main()
