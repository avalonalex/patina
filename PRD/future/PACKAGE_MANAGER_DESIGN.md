# Package Manager Design: `patina pkg`

**Status — 2026-09-12:** design proposal, not an implemented command. Library resolution already
works; [#195](https://github.com/avalonalex/patina/issues/195) tracks acquisition. Investigate the
existing Snow installer before choosing the broader fetcher proposed below. Bundling follows
[the current policy](../phase2/R7RS_LARGE_STATUS.md#bundling-policy): R7RS-large libraries,
including drafts, and SRFIs are eligible; other implementations' libraries stay external.

## #195 investigation: Snow installation layout

**Measured 2026-09-12:** Snow's `generic` target can install source libraries into a root Patina
consumes without loader changes. This establishes a local installation path, not yet a complete
end-user acquisition workflow or a reason to close #195.

Used Homebrew's `snow-chibi` / Chibi 0.12.0 and the existing `target/release/patina` (version
0.1.0), on both backends. All installs went into temporary roots, with isolated configuration and
repository cache. Archives were reconstructed from the committed corpus or original synthetic
fixtures, not downloaded release archives. Package suites were skipped during installation and
the installed libraries were exercised separately. No runtime was rebuilt for this investigation.

| Case | Installation and execution result |
|---|---|
| `(chibi irregex)` 0.9.3, whose archive has a top-level `irregex.sld` | Snow installed `chibi/irregex.sld` and `chibi/irregex.scm`; both Patina backends found `"123"` in `"abc123def"` through `irregex-search` |
| `(chibi match)` 0.9.1, with nested `include "match/match.scm"` | Installed declaration and nested include; matching `(20 22)` and adding its elements returned `42` on both backends |
| Three synthetic packages: `(example top)` → `(example middle)` → `(example leaf)` | Requesting only `top` from a local Snow repository installed all three, relocating their declarations and includes; both backends returned `42` |
| Search-path boundary | `(chibi match)` failed without its external root and returned `42` with either `-A` or `PATINA_LIBRARY_PATH`, on both backends |
| Project default | The installed dependency chain also returned `42` from `./.patina/lib/` without path flags, on both backends |

The exercised CLI shape, with `scratch` naming a temporary directory and `config.scm` containing
an isolated `input-history` path, was:

```sh
SNOW_CHIBI_CONFIG="$scratch/config.scm" snow-chibi \
  --implementations generic --always-no \
  --repo "$scratch/repo.scm" \
  --local-user-repository "$scratch/cache" \
  --install-library-dir "$scratch/installed" \
  install --skip-tests --use-sudo never '(example top)'
patina -A "$scratch/installed" "$scratch/main.scm"
```

For the two corpus packages, an archive pathname replaced the library name and an empty local
repository replaced the fixture index. Those runs establish local archive installation; they do
not establish dependency resolution against the live Snow index.

**The include relocation already exists upstream.** Snow runs `default-builder` before
`default-installer`: the builder copies includes relative to the relocated declaration and
returns updated library metadata. Verified in the installed 0.12 source and the local Chibi
checkout at `186e06597c1fbecb48c7a396df2f39c229dcc405`, then exercised by the irregex install.
`crates/patina-compat/src/run.rs`'s comment claiming Snow leaves those includes unreachable
describes the installer in isolation and needs correction. Patina's harness still needs its own
staging because it runs extracted source without invoking Snow's builder.

**Next work, before choosing a fetcher:**

1. Repeat with downloaded, pinned archives and the live index, including a real transitive
   dependency chain. Record package versions, checksums and how to reproduce an install offline.
2. Check feature-dependent metadata against Patina. `generic` is a layout target; it does not
   provide Patina's feature or bundled-library inventory. In particular, verify `(library …)`
   choices and that installation does not replace shipped SRFIs with unsuitable copies.
3. Exercise packages needing Patina adaptations. Upstream `(chibi filesystem)` has no portable
   fallback; a correct installation layout cannot supply the Patina branch maintained in
   `test-lib/`. Define how users obtain such adaptations without bundling a Chibi API.
4. Document a supported project-local workflow if these checks pass. Chibi would be an optional
   acquisition tool, with ordinary Patina execution and routine CI using installed/pinned sources
   offline. A standalone Patina fetcher is warranted if the external tool or its target model
   cannot meet that workflow; the six phases below are not prerequisites for library loading.

The existing removal of `lib/chibi/` is complete. Further cleanup should audit public APIs and
their dependencies against the policy, retaining eligible SRFI/R7RS-large libraries regardless
of where their implementations originated. Acquisition removes pressure to bundle a foreign API;
it does not make a standard library ineligible.

## Summary

The remainder is the broader `patina pkg` proposal, contingent on the investigation above.

A lightweight package manager for Patina that downloads dependencies into a local directory and adds it to the library search path. Compatible with existing Scheme ecosystems (Akku, Snow/snow-fort.org) rather than building a competing registry.

**Philosophy:** Patina doesn't need its own package registry. The Scheme world already has libraries published on Akku and Snow. The job is to make them easy to install and use.

## How It Works (User Perspective)

```bash
# Initialize a project
patina pkg init

# Install from Akku registry
patina pkg add --akku (chibi test)
patina pkg add --akku (industria crypto aes)

# Install from Snow
patina pkg add --snow (chibi uri)

# Install from git
patina pkg add --git https://github.com/someone/cool-lib.git

# Install a local path (for development)
patina pkg add --path ../my-other-lib

# Install all dependencies from manifest
patina pkg install

# List installed packages
patina pkg list

# Remove a package
patina pkg remove (chibi test)
```

After `patina pkg install`, your program just works — `(import (chibi test))` resolves automatically because `.patina/lib/` is in the search path.

## Directory Layout

```
my-project/
├── patina.pkg              # Package manifest (what you want)
├── patina.lock             # Pinned versions (what you got)
├── lib/                    # Your project's own libraries
│   └── my-app/
│       └── utils.sld
├── .patina/
│   ├── lib/                # Installed dependency .sld + .scm files
│   │   ├── chibi/
│   │   │   └── test.sld
│   │   └── industria/
│   │       └── crypto/
│   │           └── aes.sld
│   └── src/                # Downloaded source archives/repos (cache)
│       ├── akku/
│       └── snow/
└── src/
    └── main.scm
```

**Key invariant:** `.patina/lib/` contains only `.sld` and `.scm` files in the standard R7RS directory layout. No special format — any R7RS implementation could use this directory.

## Manifest Format: `patina.pkg`

S-expression format (it's a Scheme project):

```scheme
(package
  (name "my-web-app")
  (version "0.1.0")
  (description "A web application")

  (dependencies
    ;; From Akku registry
    (akku (industria crypto aes) ">=1.0.0")
    (akku (hashing sha-2) "^2.0")

    ;; From Snow
    (snow (chibi uri) ">=0.1")

    ;; From git
    (git (cool-lib utils)
      (url "https://github.com/someone/cool-lib.git")
      (ref "v1.2.0"))

    ;; Local path (for multi-project development)
    (path (my-shared-lib)
      (dir "../shared-lib/lib")))

  (dev-dependencies
    (akku (chibi test) "*")))
```

### Why s-expressions?

- Consistent with the project — this is a Scheme tool
- Parseable by Patina's existing reader (no new parser needed)
- Akku also uses s-expression manifests (`Akku.manifest`)

## Lock File: `patina.lock`

Records exact versions and checksums for reproducible installs:

```scheme
(lock-file
  (version 1)
  (packages
    ((name (industria crypto aes))
     (source akku)
     (version "1.2.3")
     (checksum "sha256:abc123..."))
    ((name (chibi uri))
     (source snow)
     (version "0.7.1")
     (checksum "sha256:def456..."))
    ((name (cool-lib utils))
     (source git)
     (url "https://github.com/someone/cool-lib.git")
     (commit "a1b2c3d4e5f6..."))))
```

## Integration with Existing Library Loading

**The search-path hook is implemented.** Current CLI order, from
`crates/patina-runtime/src/library_registry.rs` and `crates/patina-repl/src/main.rs`:

1. `-I` directories, in flag order
2. `PATINA_LIBRARY_PATH` entries
3. `./lib/` — project's own libraries
4. `./.patina/lib/` — installed project dependencies
5. `$PATINA_HOME/lib/`, if set
6. Workspace/executable-relative library roots
7. `-A` directories, in flag order
8. The script's own directory

The first matching library wins. `./lib/` and `./.patina/lib/` are relative to the working
directory; there is no project-root discovery implied here. Installation must preserve relative
includes and place declarations where their import names resolve. The loader searches `.sld`
then `.sls` within each root; R6RS source support remains bounded by the implemented bridge.

## Source Compatibility: Akku & Snow

### Akku Integration

Akku packages use R6RS and R7RS library formats. Akku's registry index is at `https://akkuscm.org/` with a GitLab-backed package database.

**Install flow:**
1. Query Akku's package index for the library name and version
2. Download the source tarball
3. Extract to `.patina/src/akku/<package>/`
4. Copy/symlink `.sld` and `.scm` files into `.patina/lib/` following the R7RS directory convention

**Compatibility notes:**
- Akku packages may include both R6RS (`.sls`) and R7RS (`.sld`) files — we only use `.sld`
- Some Akku packages use `cond-expand` for implementation-specific code — Patina supports `cond-expand`
- Packages with native/FFI dependencies won't work (document this clearly)

### Snow Integration

Snow packages ("snowballs") are tar.gz archives with a `package.scm` descriptor. The registry is at `https://snow-fort.org/`.

**Install flow:**
1. Query snow-fort.org repository index
2. Download the snowball (`.tgz`)
3. Extract to `.patina/src/snow/<package>/`
4. Copy `.sld` and `.scm` files into `.patina/lib/`

**Compatibility notes:**
- Snow packages are R7RS-native — high compatibility expected
- Snow's `package.scm` declares library paths — use these to find the right files
- Snow packages may have `(cond-expand)` blocks for different implementations

### What Won't Work

Be honest about limitations:
- **R6RS-only packages** — no `.sld` file, only `.sls` (could add R6RS→R7RS conversion later)
- **Packages requiring C FFI** — Patina has no FFI (yet)
- **Implementation-specific packages** — packages that use Chibi/Gauche/Chez internal APIs

## Global Install: `$PATINA_HOME`

For libraries you want available everywhere (like `cargo install`):

```bash
# Install globally
patina pkg install --global (chibi test)

# This puts files in $PATINA_HOME/lib/
# which is already in the default search path
```

## Phased Implementation

### Phase 1: Local path dependencies (1 week)

Minimum viable: `patina pkg init`, `patina.pkg` manifest, `--path` dependencies only.

- Parse `patina.pkg` manifest using Patina's own reader
- Use the existing `.patina/lib/` search path
- Copy/symlink local path dependencies into `.patina/lib/`
- This alone is useful for multi-project Scheme development

**Changes:**
- New crate: `patina-pkg` (CLI subcommand)
- No new search-path hook required

### Phase 2: Git dependencies (1 week)

- `patina pkg add --git <url>` clones repo, copies `.sld`/`.scm` files
- Lock file records commit hash
- Tag/branch/commit ref support

### Phase 3: Snow integration (1–2 weeks)

- Fetch snow-fort.org repository index
- Download and extract snowballs
- Map `package.scm` library declarations to `.patina/lib/` layout
- Version matching (semver-compatible)

### Phase 4: Akku integration (1–2 weeks)

- Fetch Akku package index
- Download and extract source tarballs
- Filter for R7RS `.sld` files (skip R6RS-only)
- Version resolution with semver

### Phase 5: Lock file & global install (1 week)

- `patina.lock` generation and reading
- `patina pkg install` reproduces exact versions
- `--global` flag for `$PATINA_HOME/lib/` installs

### Phase 6 (optional): Simple registry

- If the Scheme community needs it, a simple static index (JSON or s-expr on GitHub)
- Patina-native packages that aren't on Akku/Snow
- Publishing workflow

## CLI Design

Subcommand of the main `patina` binary:

```
patina pkg init                    Create patina.pkg in current directory
patina pkg add <source> <lib>      Add a dependency
patina pkg remove <lib>            Remove a dependency
patina pkg install                 Install all dependencies from manifest
patina pkg update                  Update to latest compatible versions
patina pkg list                    List installed packages
patina pkg search <query>          Search Akku + Snow registries
patina pkg clean                   Remove .patina/src/ cache
```

### `patina pkg init` output:

```scheme
;; patina.pkg — Package manifest for my-project
(package
  (name "my-project")
  (version "0.1.0")
  (dependencies)
  (dev-dependencies))
```

## Design Decisions

### Why not just use Akku directly?

- Akku requires Chez Scheme or Guile to run — heavy dependency
- Akku's `.akku/lib/` layout includes R6RS conversions we don't need
- We want `patina` to be self-contained (single binary, no external Scheme runtime)
- But we absolutely want to *consume* Akku's package ecosystem

### Why not a custom registry?

- The Scheme ecosystem is small — fragmenting it further helps nobody
- Akku has ~300 packages, Snow has ~200 — together they cover most needs
- If a package isn't on either, git is fine
- We can always add a registry later if there's demand

### Why `.patina/lib/` instead of `lib/`?

- `lib/` is for your project's own code
- `.patina/lib/` is for downloaded dependencies (like `node_modules/` or `target/`)
- Clean separation: `.patina/` goes in `.gitignore`
- `lib/` stays in version control

### Version resolution strategy

Keep it simple initially:
- Exact match: `"1.2.3"`
- Compatible (caret): `"^1.2"` → `>=1.2.0, <2.0.0`
- Minimum: `">=1.0"`
- Any: `"*"`
- No complex constraint solving in Phase 1 — if two packages need incompatible versions of the same dependency, error out. Add PubGrub-style resolution later if needed.

## Integration Points in Codebase

| What | Where | Change |
|------|-------|--------|
| Search path | `crates/patina-runtime/src/library_registry.rs` | Already searches `.patina/lib/` |
| CLI subcommand | `patina-repl/src/main.rs` | Route `pkg` subcommand |
| Package manager | New: `crates/patina-pkg/` | Manifest parsing, download, install |
| Manifest parsing | `patina-pkg` | Reuse Patina's reader for s-expr parsing |

## Open Questions

1. **Should `patina run` auto-install?** Like `npm`, detect `patina.pkg` and auto-run install if `.patina/lib/` is missing? Probably yes for convenience.

2. **Dev dependencies:** Should `patina test` automatically include dev-dependencies in the search path? Probably a separate `.patina/dev-lib/` or just mixed in.

3. **R6RS conversion:** Akku can convert R7RS→R6RS. Should we attempt R6RS→R7RS conversion for packages that only ship `.sls`? Non-trivial but would expand the usable ecosystem.

4. **Cond-expand features:** What feature identifiers should Patina declare? At minimum: `r7rs`, `patina`, and platform features (`posix`, `darwin`, `linux`, `windows`).
