# PRD/study

Dated research records behind a PRD. Each subdirectory keeps one study as it stood when it closed: the reports,
the design drafts they produced and the small probe programs they ran.

These records are not maintained. Their code references, line numbers and measurements belong to the commit and the
date each study names, and nobody updates them as the code changes. The PRD a study fed is authoritative: where a
record and its PRD differ, the PRD holds. New findings go in GitHub issues, not here (AGENTS.md, "PRDs are high level;
work items are GitHub issues").

| Study | Dates | Revision | Authoritative document |
|---|---|---|---|
| [`gc/`](gc/README.md): garbage collector redesign, threading model, cost of future shared-memory parallelism, steady state and limits | 2026-09-30 to 2026-10-01 | `main` at `28a94f8` | [`PRD/GC_PRD.md`](../GC_PRD.md) |
