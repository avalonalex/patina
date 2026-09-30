# Editor Integration and Scheme Formatting

**Status:** Deferred idea. No implementation or delivery milestone is scheduled.
**Decision:** 2026-09-30 — preserve the direction; it is too early to pursue.
**Origin:** [Reader audit #370](https://github.com/avalonalex/patina/issues/370),
“Considered and not planned.” The audit's completed work can remain closed.

## Purpose

Eventually make Patina comfortable to use from an editor, with source diagnostics,
structural navigation and Scheme formatting that preserves the program's meaning
and comments. A standalone formatter could be useful before a full editor
integration. The choice of editor, protocol and formatting style remains open.

## Why this is deferred

The reader audit addressed interpreter robustness, conformance and diagnostics.
Editor support introduces different requirements: partially written forms,
repeated edits, comment placement and source spelling. There is no current editor
or formatter requirement that justifies building that infrastructure now.

Source spans and retained source documents from
[#367](https://github.com/avalonalex/patina/issues/367) provide a useful foundation.
They do not preserve the complete written structure: the reader skips whitespace
and comments while constructing datums. Printing those datums cannot serve as a
source formatter because it loses comments and choices of spelling.

## Ideas to revisit

- **A lossless source representation.** Retain tokens, comments, whitespace and
  original spellings alongside syntax structure. Unedited source should be
  reproducible exactly. Formatting may change layout while keeping comments,
  directives, datum labels and literal meanings intact.
- **Editor handling of unfinished input.** Represent incomplete or invalid forms
  well enough to show diagnostics and support navigation while a person types.
  Decide whether error recovery and several diagnostics per file are useful for
  that workflow when it exists.
- **Incremental reparsing.** Reuse unaffected syntax after an edit if measurements
  show whole-document reparsing cannot meet the editor's response-time needs.
  Reading a port incrementally is a separate capability from reparsing edits to
  an existing document.
- **Readtables.** Revisit configurable reader syntax only if a concrete tooling or
  language-extension use case requires it. Editor integration and formatting do
  not by themselves require extensible reader syntax.

Keep tooling aligned with Patina's reader grammar, dialect settings and numeric
extensions. Assess any extra memory and processing cost against the existing
streaming reader and ordinary execution paths. Macro-aware navigation and
indentation need their own scope decisions; recognizing a spelling alone does not
establish which binding or macro it denotes.

## When to resume

Revisit this idea when an actual editor workflow or formatter becomes a project
priority. Choose one initial user-facing capability, measure its needs, and then
decide which representation and parsing changes it warrants. Formatting should
preserve readable data and be idempotent; edited-source parsing should agree with
a fresh parse once the input is complete.

Concrete investigations and implementation work belong in GitHub issues when
activated. This note preserves the direction without adding work to the completed
reader audit. Runtime stepping remains a separate future idea in the
[visual debugger design](VISUAL_DEBUGGER_DESIGN.md).
