# Documentation Map

**Last Updated:** 2025-11-09

Quick reference guide to find the right documentation for your needs.

---

## 🎯 Quick Links - Start Here

| What You Need | Document |
|---------------|----------|
| **What's implemented?** | [`docs/FEATURE_STATUS.md`](docs/FEATURE_STATUS.md) ⭐ |
| **What's next?** | [`PRD/phase1/IMPLEMENTATION_STATUS.md`](PRD/phase1/IMPLEMENTATION_STATUS.md) ⭐ |
| **Getting started as user** | [`docs/GETTING_STARTED.md`](docs/GETTING_STARTED.md) |
| **Contributing as developer** | [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) |
| **Running tests** | [`docs/TESTING.md`](docs/TESTING.md) |
| **Using the API** | [`docs/API.md`](docs/API.md) |

---

## 📂 Documentation Structure

### 📁 **docs/** - User & Developer Guides

**Purpose:** User-facing documentation for using Patina and contributing to development

| Document | Description |
|----------|-------------|
| **FEATURE_STATUS.md** ⭐ | Detailed test-by-test R7RS compliance matrix (updated regularly) |
| API.md | Public Rust API reference for embedding Patina |
| GETTING_STARTED.md | User guide for installation and basic usage |
| README.md | Project overview and quick introduction |
| TESTING.md | Test infrastructure, running tests, writing tests |
| DEVELOPMENT.md | Developer setup, architecture, contribution guide |

---

### 📁 **PRD/phase1/** - Strategic Planning

**Purpose:** High-level design documents and strategic roadmaps for current phase

| Document | Description |
|----------|-------------|
| **IMPLEMENTATION_STATUS.md** ⭐ | Overall roadmap, priorities, remaining work |
| **R7RS_ROADMAP.md** | Strategic R7RS compliance planning |
| **NUMERIC_SUMMARY.md** ⭐ | Canonical guide for numeric tower implementation |
| **DEBUGGER_HOOK_SYSTEM.md** | Debugger and runtime hook system design |
| **IO_IMPLEMENTATION.md** | Complete I/O system implementation guide (50+ procedures) |
| **EXCEPTION_HANDLING.md** | Exception handling implementation guide |
| STRING_OPTIMIZATION.md | Future string optimization plans (deferred) |
| HELP_SYSTEM.md | Future help system design (deferred) |

**Note:** Only current phase (phase1) is active. Future phases in PRD/future/

---

### 📁 **internal/** - Implementation Notes

**Purpose:** Implementation notes, progress tracking, and active limitations

| Document | Description |
|----------|-------------|
| **MILESTONES.md** | Historical achievements and progress (keep updated) |
| NESTED_ELLIPSIS_LIMITATION.md | Future enhancement: nested ellipsis support |

#### 📁 **internal/reference_impls/** - Reference Implementation Notes

| Document | Description |
|----------|-------------|
| CHIBI_REFERENCE.md | Chibi-scheme implementation notes |
| CHEZ_REFERENCE.md | Chez Scheme implementation notes |

---

### 🗄️ **internal/ARCHIVE/** - Completed Research

**Purpose:** Historical documentation for completed features (read-only reference)

See [`internal/ARCHIVE/README.md`](internal/ARCHIVE/README.md) for full details.

| Directory | Contents | Status |
|-----------|----------|--------|
| `macro_research/` | 9 research docs on macro implementation | ✅ COMPLETE (2025-11-08) |
| `numeric_research/` | 7 research docs on numeric tower | ✅ 94% COMPLETE |
| `completed_features/` | Do loop, macro system summaries | ✅ COMPLETE |
| `historical/` | Resolved bugs and issues | Resolved |

**Total:** 20 archived documents

**When to use:** Historical context, understanding design decisions, implementing related features

---

## 🔍 Finding Information

### "How do I...?"

**Get started using Patina:**
→ [`docs/GETTING_STARTED.md`](docs/GETTING_STARTED.md)

**Set up for development:**
→ [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)

**Run tests:**
→ [`docs/TESTING.md`](docs/TESTING.md)

**Check what's implemented:**
→ [`docs/FEATURE_STATUS.md`](docs/FEATURE_STATUS.md) (test-by-test matrix)

**See what's next:**
→ [`PRD/phase1/IMPLEMENTATION_STATUS.md`](PRD/phase1/IMPLEMENTATION_STATUS.md) (priorities)

**Understand numeric tower:**
→ [`PRD/phase1/NUMERIC_SUMMARY.md`](PRD/phase1/NUMERIC_SUMMARY.md) (canonical guide)

**Look up project history:**
→ [`internal/MILESTONES.md`](internal/MILESTONES.md) (achievements)

**Research macro implementation:**
→ [`internal/ARCHIVE/macro_research/`](internal/ARCHIVE/macro_research/) (historical)

**Research numeric implementation:**
→ [`internal/ARCHIVE/numeric_research/`](internal/ARCHIVE/numeric_research/) (historical)

---

## ⭐ Canonical Sources of Truth

When there's conflicting information, these are the authoritative sources:

1. **R7RS Compliance Status:** `docs/FEATURE_STATUS.md`
2. **Overall Roadmap & Priorities:** `PRD/phase1/IMPLEMENTATION_STATUS.md`
3. **Strategic Planning:** `PRD/phase1/R7RS_ROADMAP.md`
4. **Numeric Tower:** `PRD/phase1/NUMERIC_SUMMARY.md`
5. **Historical Progress:** `internal/MILESTONES.md`

---

## 📊 Current Status Summary (2025-11-09)

```
Test Coverage:   395 tests passing (88% R7RS compliance)
Core Language:   100% complete (all special forms, bindings, iteration)
Macro System:    96% complete (only nested ellipsis missing)
Data Structures: 100% complete (lists, vectors, strings)
Numeric Tower:   94% complete (missing transcendental functions)
I/O:             0% (not started)
```

**Recent Achievements:**
- ✅ Macro system complete (2025-11-08)
- ✅ Do loop implemented (2025-11-09)
- ✅ Documentation consolidated (2025-11-09)

**Next Priorities:**
1. Tail Call Optimization (HIGH - R7RS requirement)
2. Advanced math functions (MEDIUM - completes numeric tower)
3. Library system (HIGH - module organization)

---

## 🧹 Maintenance

### Active Docs (Update Regularly)
- `docs/FEATURE_STATUS.md` - Update after implementing features
- `PRD/phase1/IMPLEMENTATION_STATUS.md` - Update priorities/roadmap
- `internal/MILESTONES.md` - Add achievements when major features complete

### Archive (Read-Only)
- `internal/ARCHIVE/` - Do NOT update, preserve for history

### Future Work
- `PRD/phase1/*.md` (HELP_SYSTEM, STRING_OPTIMIZATION) - Update when ready to implement

---

## 📝 Documentation Principles

1. **Single Source of Truth** - Avoid duplicating information
2. **Clear Ownership** - Each topic has one canonical document
3. **Archive Completed Work** - Reduce cognitive load
4. **Update Regularly** - Keep status docs current
5. **Preserve History** - Archive instead of deleting

---

For questions about documentation structure, see CLAUDE.md or ask the maintainers.
