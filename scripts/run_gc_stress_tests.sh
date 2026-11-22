#!/usr/bin/env bash
# The per-PR GC stress lane (#626; docs/TEST_ORGANIZATION.md, "GC lanes"): the
# cargo test targets that exercise the collector, control flow and library
# loading, under PATINA_GC_STRESS=16 in a build with the stale-reference
# checks, and the suite files' driver, scheme_suite.rs, at 4096. The chibi
# lanes reach the collector only through the CLI and only with the chibi
# suite's programs; these targets drive it through the embedding API
# (Interpreter, Backend::eval, library loading from Rust) and with the shapes
# a GC bug has needed: an ephemeron holding a continuation (#130), an escape
# from a primitive, a continuation re-entered after a collection.
#
# Each target must pass, and must report at least the number of collections
# pinned for it below: a lane that passes without collecting has tested
# nothing, and four have (#5, #164, #200, #201). The count is the record
# PATINA_GC_COUNT_DIR asks for (crates/patina-core/src/heap/gc.rs), one per
# process, which must also say the process ran under this interval. A
# minimum is half the count measured at its interval when it was pinned, or,
# where a target's zeal halves keep its count without stress above that,
# midway between its counts without stress and at its interval (the counts
# are deterministic), so every one is above what the target makes without
# stress, and a run the variable did not reach fails here even if the record
# went missing some other way. Lower a minimum only with a measurement.
#
# Each target must also run all its tests: none filtered out, and at least
# the number pinned below, which is how many it has. A run cut short or
# filtered still passes and can still collect, and a GC lane passed that way
# once, with the chibi suite run two assertions deep (#201). Raise a pin when
# a target gains tests; lower one only with the tests it lost.
#
# library_loading and macro_definition_env do nearly all their work inside
# library loads, which deferred collection for as long as a library's
# unevaluated body existed (ParsedLibrary's GcDeferGuard::holding), so
# library_loading's minimum was near zero. Since #677 a load the backend runs
# from outside any loop is rooted by its registry entry and collects inside
# its body and where it ends, so under stress these two collect inside their
# loads, and with rooted_loading they are what catches a root the loading
# entries miss.
#
# scheme_suite.rs runs every tests/scheme file on both backends, and at 16 it
# did not finish in 25 minutes (#626). One file is nearly all of that:
# srfi/regex-graphemes.scm, which compiles SRFI 115's grapheme regex, a large
# live heap of character sets, and then matches it against 400 syllables. Run
# alone through the debug CLI it took 9 s on the VM and 22 s on the
# tree-walker without stress, 12 s and 28 s at 4096, 58 s and 81 s at 256,
# and had not finished after 300 s on either at 16; every other file together
# took about 90 s and 130 s at 16. The whole target took 20 s without stress,
# 27 s at 4096, 47 s at 1024 and 125 s at 256 (measured 2026-10-02, under
# other load), so it runs at 4096, where it collects six times as often as
# it did without stress for a third more time. Since the byte trigger (#606)
# it collects less without stress, and 4096 is 28 times as often.
#
# Run it against a check build: a debug build, as here, or release with
# `--features patina-tests/gc-check` passed through. CI's Test Suite job
# runs it after `cargo test --all --lib --tests`, whose build it reuses: it
# names the workspace (`--workspace`), as that command does, so the features
# resolve the same and nothing recompiles.
#
# Usage: scripts/run_gc_stress_tests.sh [extra cargo test arguments]
# (a build's, such as `--release --features patina-tests/gc-check`; a test
# filter fails the tally)
set -euo pipefail

cd "$(dirname "$0")/.."

# This lane sets the one GC variable it means; one left exported in the
# shell would change what it measures.
unset PATINA_GC PATINA_GC_STRESS PATINA_GC_ZEAL PATINA_GC_COUNT_DIR
# Descriptor pressure posts a collection once min(128, soft RLIMIT_NOFILE / 4)
# file ports opened since the last one are still open (#607), so the soft
# limit decides when those collections come. Pin the threshold at 128, as
# any limit of 512 or more gives it: raise a lower one, such as the 256 a
# macOS shell starts with, to 1024, and leave a higher one as it is.
nofile=$(ulimit -Sn)
if [ "$nofile" != unlimited ] && [ "$nofile" -lt 512 ]; then
    ulimit -Sn 1024 || {
        echo "error: cannot raise the descriptor limit from $nofile to 1024 (hard limit $(ulimit -Hn))" >&2
        exit 1
    }
fi

# target, interval, minimum collections, tests (measured 2026-10-02: the
# minimum is half the count at that interval; in parentheses the count, and
# the count without stress, re-measured 2026-10-03 under the byte trigger,
# #606, which left every count under stress as it was; gc_tree_walker's and
# gc_vm's counts re-measured that day too, when their reclamation proofs
# moved to bytes, which raised them slightly, and again when the arena
# comparison stopped making a third run, which lowered them; ephemerons'
# minimum re-pinned 2026-10-04, when #639's tests of `(gc)` collecting at its
# call doubled its count, and again that day for seven more of them, `gc`
# as a control primitive's thunk among them; escape_from_primitive's count
# re-measured that day, 728 when it was pinned, and its minimum re-pinned
# that day too, for #607's test of opens that collect and retry, which
# collect without stress as well, and again for `load`'s retry, which that
# test also re-enters; interpreter_api's re-pinned 2026-10-06, when #643
# deleted its test of the source map's pruning, with the store it measured,
# and again that day for #612's test of a hundred `eval`s of one datum;
# hygiene_matrix's test count re-pinned 2026-10-07 for #652's def-getter
# shape, its minimum left at 400; owned_handles pinned 2026-10-09, with
# #605's handles, and host_environments that day, with #620's, and again
# that day for the VM's evaluating in a host environment; vm_callprimitive's
# re-pinned 2026-10-10 for #673's two tests of a host's definitions, and that
# day library_loading's and macro_definition_env's, when #677 let library
# loads collect, macro_definition_env's test count with it, which had stayed
# at 15 as the target gained tests, and rooted_loading pinned, then re-pinned
# that day for its three tests of a bare top-level import; cps_features's and
# owned_handles's re-pinned that day too, for #680's tests of where the
# tree-walker binds its temporaries; gc_tree_walker's and gc_vm's test
# counts re-pinned 2026-10-26 for #608's test of a current port whose slot
# was reused, and their counts re-measured that day, its 600 `(gc)`s among
# them, the minimums left as they were; gc_vm's and callability's test counts
# re-pinned 2026-10-27 for #784, the comparator `member` control moving into
# the shared tests and an uncaught install's undo joining callability, all
# three counts re-measured, callability's now with fresh interpreters
# bootstrapping under stress, the minimums left; callability's re-measured
# again that day for its handler count and the rewritten Scheme loop;
# escape_from_primitive's test count re-pinned 2026-10-29 for #785's test of
# where a dead file port is written out, which sets its own modes and adds 18
# collections either way, and its counts re-measured that day: the other 13
# tests' 6334, up from 960 since #677 let library loads collect (2026-10-10),
# which no re-pin had recorded; the minimum left at half the count from
# before then; callability's test count re-pinned 2026-10-31 for #817's test
# of #781's own shape uncaught, and its count re-measured that day, the
# minimum left; escape_from_primitive's and finished_forms_release_code's
# test counts re-pinned 2026-11-01 for #779's tests of a library body form
# re-entered after its load and of the code a library's body forms and a
# host environment's forms let go, and their counts re-measured that day,
# the minimums left; host_environments's test count re-pinned 2026-11-03 for
# #830's test that a host environment is charged as a frame, and its counts
# re-measured that day, the minimum left; owned_handles's and
# interpreter_api's test counts re-pinned 2026-11-05 for #831's tests that
# `gc_stats()` reads as `(gc-stats)` does and that `display_notation` writes
# as `display` does, and their counts re-measured that day, the minimums
# left: interpreter_api's 27 other tests made 2230, up from 277 since its
# pin of 2026-10-06, which no re-pin had recorded, and the new test 47;
# host_environments's test count and minimum re-pinned 2026-11-06 for #835's
# seven tests of what host code reads and writes, what its closures keep and
# what its macros introduce, the minimum at half its count re-measured that
# day, as #835 asks; hygiene_matrix's and host_environments's test counts
# and minimums re-pinned 2026-11-06 for #834's tests of a def-getter shape
# that collects between its forms and of an import into a host environment,
# the minimums at half their counts re-measured that day, as #834 asks:
# hygiene_matrix's 14 other tests made 8628, up from 804 since its pin of
# 2026-10-07, which no re-pin had recorded, and the new test 55;
# escape_from_primitive's test count and minimum re-pinned 2026-11-06 for
# #823's tests of what a library body's interaction environment and
# one-argument `load` are and of a program form resumed during an import,
# the minimum at half its count re-measured that day, as #823 asks: the 15
# other tests' 6951 unchanged by the removal of the VM's globals swap, and
# the new tests 387; hygiene_matrix's and host_environments's test counts
# and minimums re-pinned 2026-11-06 again for #834's binding records:
# hygiene_matrix for its test of an introduced definition's record, its
# identity and its scope, which runs the test binary again in twelve child
# processes, a shape and a backend each, that collect under stress too, and
# host_environments for its tests of what a host reads through a record and
# that a read of an unbound name makes none, the minimums at half their
# counts measured that day: hygiene_matrix's 12334, up from 8683, and
# host_environments's 114484, up from 113904; host_environments's,
# hygiene_matrix's and vm_callprimitive's test counts and minimums re-pinned
# 2026-11-07 for #835's host-namespace kind: host_environments for its test
# that a library macro used in a host namespace keeps its import, its 22
# other tests making 114596, up from 114484, and the new test 250;
# hygiene_matrix for its test that every shape answers in a host namespace
# what it records, 9939 of its 22273; and vm_callprimitive for its test of
# a `set!` through a host namespace, its 17 other tests making 1020, up from
# 67 since its pin of 2026-10-10, which no re-pin had recorded (already 1020
# at the kind's base, a69bf155), and the new test 71; the minimums at half
# their counts measured that day; host_environments's test count and minimum
# re-pinned again that day for #835's test that a library macro used in a
# host namespace that imports nothing keeps the global environment's import,
# its 23 other tests making 114846 and the new test 242, the minimum at half
# its count measured that day; host_environments's test count and minimum
# re-pinned again that day for #835's test that the VM refuses a raw frame,
# its 24 other tests making 115088 and the new test 166, the minimum at half
# its count measured that day; host_environments's test count and minimum
# re-pinned 2026-11-07 again for #853's test that the VM's Scheme `eval` and
# `load` refuse a specifier over a raw frame, its 25 other tests making
# 115254, as before, and the new test 207, the minimum at half its count
# measured that day, its count without stress 125, as before; and again that
# day for #853's review, which gave that test the frame of temporaries'
# `define` and `load` and the refusal's whole message, 222 collections, and
# added the test that a host primitive's `Step::Eval` and
# `ApplyContext::eval_expr` in a raw frame are refused, 112, its 25 other
# tests making 115254 and its count without stress 125, as before, the
# minimum at half its count measured then; gc_vm's and gc_tree_walker's test
# counts and minimums re-pinned 2026-11-07 for #836's tests of a forward
# reference across collections, through a closure and through a captured
# continuation (two programs, one whose code the continuation holds in a
# closure and one in a frame alone), each counting `binding-records` and run
# on the backend's interpreter and on a zeal one, which must have collected
# beyond the program's `(gc)`s, and for `collection_is_recorded_in_stats`'s
# check that the zeal interpreter collects across 100 iterations that
# allocate nothing: a zeal interpreter collects at every outermost safe point
# whatever the lane's variable says, and the record counts it, so their
# counts without stress rose with their counts under it, gc_vm's from 667 to
# 1285 and gc_tree_walker's from 658 to 3358; at 16, gc_vm's 25 tests made
# 17584 at the base, #856's head, and gc_tree_walker's 20 made 24318, up from
# 17314 and 24151 since their counts of 2026-10-27 and 2026-10-26, which no
# re-pin had recorded; `collection_is_recorded_in_stats` now makes 197 on the
# VM and 1050 on the tree-walker, up from 35 and 34, and the two new tests
# 185 and 390 on the VM and 550 and 1252 on the tree-walker;
# host_environments's test count and minimum re-pinned again that day for
# #836's test of a host's forward references across collections, on the
# backend's interpreter and on a zeal one, its 27 other tests making 115588,
# as before, and the new test 382, its count without stress 431, up from 125;
# gc_vm's and gc_tree_walker's test counts and minimums re-pinned again that
# day for #836's tests of `(gc-stats)`'s `code-units`: on the VM, that it
# does not grow across `eval`s of fresh forms a collection lets go, and that
# a unit a live closure keeps is counted until the closure dies, the second
# run on a zeal interpreter too, the first on one and on a collection-off
# one; on the tree-walker, that it reads 0 throughout the same shapes, on a
# zeal interpreter too. Their other tests made 18321 and 27136, as before,
# and 1285 and 3358 without stress; the new tests 2013 on the VM (1793
# without stress) and 3164 on the tree-walker (3100), whose test runs ten
# `eval`s where the VM's runs fifty, since fifty made 12908 (12780): the
# zeal interpreters collect at every outermost safe point whatever the
# lane's variable says; and again 2026-11-08 for that PR's review, which
# gave the VM's tests a reading after a single `(gc)`, fifty more `eval`s
# on each of the steady test's three interpreters, wrote each test's
# `(gc)`s where they run, and added a shared test that `(gc-stats)` reports
# each per-owner count once, in the steady-state lane's order: the other
# tests made 18321 and 27136, as before, and 1285 and 3358 without stress;
# the `code-units` tests 2661 on the VM (2391 without stress) and 3148 on
# the tree-walker (3084), and the shared test 38 on each (0);
# the minimums at half their counts measured that day; gc_vm's test count
# re-pinned again that day for #836's control of `release_dead_code`'s
# check, a unit a collection kept whose link table it did not trace, which
# panics at its first collection with the switch on: 35 collections at 16
# and 1 without stress, the other 30 tests making 21020 and 3676, as before,
# the minimum left; and again 2026-11-09, when that control became two, one
# for each way a unit is kept, each making 35 collections at 16 and 1
# without stress, the other 30 tests' counts as before, the minimum left;
# and 2026-11-09 for #836's tests once the VM links each unit at load:
# gc_vm's test count re-pinned for the control of a placeholder a kept
# unit's table links that a collection drops (36 collections at 16, 1
# without stress), the shared tests that a placeholder no live code links is
# dropped (2069 and 1020 on the VM, whose zeal half runs 100 `eval`s) and
# that a read that missed is not remembered (185 and 150), and the test that
# a bound placeholder is never dropped (242 and 198), the other 32 tests
# making 21089 and 3678, the linking having moved them by one at 16;
# gc_tree_walker's for the two shared tests, 11692 and 10884 for the drop
# test, whose zeal half's 100 `eval`s collect at every outermost safe
# point, and 504 and 470, the other 24 tests' counts as before;
# host_environments's for the tests that a host namespace's placeholders
# go with their code (925 and 12) and that a host's read that missed is not
# remembered (378 and 307), `a_read_makes_no_record`'s second half adding
# 20 and 4, the other 27 tests' counts as before; and
# finished_forms_release_code's for 4b's start decision 3's gate (236 and
# 2), the other 11 tests making 3845 and 13; the four minimums at half
# their counts measured that day. vm_callprimitive's count re-measured that
# day for its host definitions' compiled `define`, 280 collections at 16
# where that test made 138, its test count and minimum left; the linking
# itself moved the other targets' counts by at most 20 (hygiene_matrix's),
# their minimums left; and every target's count re-measured that day,
# twelve of which had moved since their pins with no re-pin recording it,
# as the lane reads at the linking's base, cf7228ea (each count there, and
# the count its comment gave): callability's (2195, 1543),
# control_flow_matrix's (4826, 680), cps_features's (1014, 1060),
# ephemerons's (5894, 578), escape_from_primitive's (9950, 7338),
# finished_forms_release_code's (3844, 3677), interpreter_api's (3128,
# 2277), library_loading's (131, 93), macro_definition_env's (33675,
# 33253), owned_handles's (125668, 125485), rooted_loading's (1283, 742)
# and scheme_suite's (10230, 3068); host_environments's count re-measured
# 2026-11-10, when its test of a host namespace's placeholders took a
# reading after a single `(gc)`, 929 collections at 16 where it made 925
# and 12 without stress, as before, its test count and minimum left;
# gc_vm's test count re-pinned 2026-11-11 for #836's two tests that a
# collection's L leaves out the namespaces its code release drops: the
# paced one runs on an interpreter that collects at the adaptive threshold
# whatever the lane's variable says, 6 collections either way, and the one
# at `(gc)`'s call 160 at 16 and 15 without stress, the other 36 tests
# making 23621 and 5047, as before, the minimum left; and again that day
# for the test of the collection where a library's load ends, which runs
# on the adaptive interpreter too, 4 collections either way, the other 38
# tests making 23787 and 5068, as before, the minimum left; gc_vm's and
# gc_tree_walker's test counts re-pinned 2026-11-11 for #836's five shared
# tests of code that runs after the namespace it was compiled in has no
# other holder, 964 collections at 16 on the VM and 772 without stress,
# nearly all their zeal halves, and 2898 and 2714 on the tree-walker, the
# other tests making 23791 and 5072, and 42518 and 17796, as before, the
# minimums left; and every target's count at 16 re-measured that day for
# #836's deletions, which make one allocation fewer for each library body
# form, host namespace's form and nested evaluation (the retired frame
# closure), so each target that loads a library collects a little less
# often, by up to 5% (vm_callprimitive), its tests, its count without
# stress and its minimum left; gc_vm's, gc_tree_walker's and
# escape_from_primitive's test counts re-pinned again that day for that
# PR's review: two shared tests, of a closure only an ephemeron's datum
# holds and of a replaced library's procedure that reads by name through
# its namespace, which raised the counts by 373 collections at 16 on the
# VM and 298 without stress, and by 1076 and 1000 on the tree-walker,
# nearly all their zeal halves; and a test of a closure a re-entered
# library body form makes, which raised escape_from_primitive's by 76 and
# 2. gc_tree_walker's minimum re-pinned at half its count measured that
# day, since its count without stress, 21510, had passed the minimum of
# 21259; the others left; host_environments's test count re-pinned
# 2026-11-12 for #836's test of a host primitive that evaluates in a host
# namespace, which raised its count at 16 by 72 and left the count without
# stress at 754, the minimum left; and gc_vm's and gc_tree_walker's test
# counts that day for #836's shared test of a procedure that drops itself
# and tail-calls, whose unit only the running code reports at the callee's
# entry, which raised the counts by 155 collections at 16 on the VM and 122
# without stress, and by 421 and 388 on the tree-walker, nearly all its
# zeal halves, the minimums left; and macro_definition_env's and
# vm_callprimitive's test counts re-pinned 2026-11-13 for 4b.10's tests
# (#837) of what programs answer through a template's references across
# forms, which pass before and after its links: four in
# macro_definition_env, which raised its count at 16 from 33641 to 33931
# and without stress from 67 to 75, and two in vm_callprimitive, from 1174
# to 1318, and 0 without stress, the minimums left; then, for the review's
# follow-up, a fifth in macro_definition_env, a form settling rebuilds,
# which raised its count at 16 to 34001 and left it at 75 without stress,
# and vm_callprimitive's fast-path test's look at the code it compiles,
# which raised its count at 16 to 1350, the minimums left; and
# macro_definition_env's test count re-pinned 2026-11-14 for 4b.10's switch
# (#837), three tests that an early-bound reference mints no spelling,
# leaves the program's names alone and is not captured by a definition of
# the spelling an alias had, which raised its count at 16 to 34214 and left
# it at 75 without stress; vm_callprimitive's two operand checks raised its
# count at 16 to 1382 (0 without), and template-references.scm's two rows,
# which define and read 10,000 names, raised scheme_suite's at 4096 to 10572
# and without stress to 385, the minimums left; and hygiene_matrix's test
# count re-pinned 2026-11-14 for the review's test of what the scope trace
# says of an early-bound reference, linked and settled, whose children run
# under the trace, which raised its count at 16 from 21652 to 21790 and
# left it at 6 without stress, the minimum left; and macro_definition_env's
# test count re-pinned 2026-11-15 for 4b.10's keyword aliases (#837), two
# tests that a keyword alias holds its target's record and names no
# environment, and that the record still keeps a replaced library's
# namespace, which raised its count at 16 from 34214 to 34428 and without
# stress from 75 to 83, the minimum left; then, for the review's
# follow-up, a third, that a keyword alias to an imported keyword releases
# the replaced library, which raised its count at 16 to 34504 and without
# stress to 87, the minimum left; and gc_vm's and gc_tree_walker's test
# counts and minimums re-pinned 2026-11-16 for #838's three shared tests of
# a redefined library's release, and each file's test of a library body
# whose code quotes its own procedure, which raised the counts at 16 from
# 25199 to 39118 on the VM and from 46913 to 82199 on the tree-walker, and
# without stress from 6264 to 17266 and from 21898 to 54236, nearly all
# their zeal halves: both counts without stress had passed their minimums,
# so gc_vm's minimum is half its count at 16, and gc_tree_walker's, whose
# count without stress is above that half, midway between its two counts,
# as the rule at the top now says; then, for #838's mutation matrix, a
# holder row that a macro's definition environment alone keeps and two
# checks in the release test's reading, which raised the counts at 16 to
# 39361 and 83012 and without stress to 17464 and 55000, the minimums taken
# by the same rule; host_environments's test count and minimum re-pinned
# 2026-11-16 for #839's two tests on `Backend`'s handle methods, a held raw
# namespace across a collection and a tree-walker whose global is a frame,
# and the `eval_in` routes its #620 tests and raw frames gained, which
# raised its count at 16 from 117297 to 167613 and without stress from 753
# to 761 (the base re-measured at be9c1701, where the row read 117298 and
# 754), the minimum at half its count at 16; gc_vm's edited test, which
# reads its two counts in forms of one shape, left its counts as they were,
# and so did vm_callprimitive's, owned_handles's,
# finished_forms_release_code's, macro_definition_env's and
# hygiene_matrix's, whose tests did not change: a handle allocates nothing
# in the arenas; then vm_callprimitive's count re-measured that day for a
# second row of its host-definition test, through the deprecated
# `Backend::define`, which raised it at 16 from 1382 to 1455, without stress
# 0 as before, its test count and minimum left; host_environments's
# frame-global test's new checks left its counts as they were; and four
# targets re-pinned 2026-11-16 for the scoreboard of #837's second PR, rows
# of the relinking shapes that pass before it: macro_definition_env's test
# count for three (three levels of generated macro, two libraries, a
# redefinition), which raised its count at 16 from 34504 to 34738 and
# without stress from 87 to 91, the minimum left; gc_vm's and
# gc_tree_walker's test counts for the shared test of a literal-only
# generated macro across its library's redefinition, which raised the
# counts at 16 to 39552 and 83551 and without stress to 17616 and 55500,
# the minimums taken by the same rule; and scheme_suite's count at 4096,
# which expansion/relinked-names.scm and a row of splicing-libraries.scm
# raised from 10572 to 10648 and left at 385 without stress, its test count
# and minimum left; and the same four re-pinned 2026-11-17 for the
# scoreboard's second batch, so that each kind of relink target is held in
# more than one target before the switch: macro_definition_env's test count
# for two (a definer's introduced state and a splice's private keywords,
# each across forms and collections), which raised its count at 16 from
# 34738 to 34917 and without stress from 91 to 107, the minimum left;
# gc_vm's and gc_tree_walker's test counts for the shared test of a
# generated macro that keeps a splice's keyword across its library's
# redefinition, which raised the counts at 16 to 39737 and 84050 and
# without stress to 17762 and 55960, the minimums taken by the same rule;
# and scheme_suite's count at 4096, which four rows of
# expansion/relinked-names.scm and one of splicing-libraries.scm raised
# from 10648 to 10650 and left at 385 without stress, its test count and
# minimum left; and four test counts re-pinned 2026-11-17 for the switch's
# first step (#837), a relinked reference linked to its record:
# hygiene_matrix's for two (what the scope trace says of a relinked
# reference, linked and settled, and the record every relinking writes,
# whose children run under the trace), which raised its count at 16 from
# 21790 to 22238 and left it at 6 without stress; vm_callprimitive's for a
# relinked operand's printed spelling, from 1455 to 1565, and 0 without;
# macro_definition_env's for two (a definition of a relinked spelling
# captures nothing; a relinked keyword used as a value is named as
# written), from 34917 to 35062, and 107 without; and host_environments's
# for code compiled before a host's nearer definition, from 167613 to
# 167687, and 761 without; the minimums left. The step itself moved no
# count: gc_vm, gc_tree_walker and scheme_suite read as they did; and the
# same re-pinned that day for its second step, a generated macro carrying
# its relinks across forms: vm_callprimitive's test count for the carried
# reference's operand, which raised its count at 16 to 1669, and 0
# without; macro_definition_env's for three (a definition of a carried
# spelling, a generated definer run twice, a relink of each kind in one
# macro), to 35302, and from 107 to 113 without; host_environments's for
# the keyword a generated macro carried past a host's definition, to
# 167767, and 761 without; hygiene_matrix's trace test's fourth program
# raised its count at 16 to 22310, its test count left; and
# relinked-names.scm's row for the generated definer raised scheme_suite's
# at 4096 to 10652 and left it at 385 without; the minimums left; and for
# the switch itself, from which relinking installs no alias and interns
# nothing: macro_definition_env's test count for one more (the alias scan
# of a definition over the spelling went, its successor having landed with
# the first step; a relinked spelling is no name in a program's text; a
# template that writes it unbound reads nothing, which runs alone in a
# child for the range it writes), which raised its count at 16 to 36200,
# and to 115 without; hygiene_matrix's for a program's identifier spelled
# as a relinked literal, to 22388, and 6 without; and six rows of
# relinked-names.scm raised scheme_suite's at 4096 to 10653 and left it at
# 385 without; the minimums left. The switch moved no count of a target
# whose tests it left alone: gc_vm, gc_tree_walker, host_environments and
# vm_callprimitive read as they did; and six targets re-pinned that day for
# the rest of the switch's tests: gc_vm's and gc_tree_walker's test counts
# for the relinked twin of a value kept from a replaced library, and their
# shared tests' three relinking variants of a redefined library's release
# (800 redefinitions each, under zeal too) and four holder rows, which
# raised the counts at 16 from 39737 to 58197 and from 84050 to 129201 and
# without stress from 17762 to 31094 and from 55960 to 95958: both counts
# without stress are above half their counts at 16, so each minimum is
# midway between its two counts; macro_definition_env's test count for
# twelve (no spelling minted, the program's names left alone, only an
# identifier of the expansion, a template writing bound names around the
# spelling, the four relinked twins of the early-link tests, a definition
# an expansion produces into a body and at top level, and the body scan's
# two), which raised its count at 16 to 39531, and to 127 without;
# hygiene_matrix's for three relinked-literal tests, to 22610, and 6
# without; host_environments's for a relinked expansion in a host
# namespace, to 167847, and to 765 without; and vm_callprimitive's for the
# code of a relinked apply, call-with-values and assignment, to 1893, and 0
# without; those four minimums left; and hygiene_matrix's test count
# re-pinned 2026-11-18 for #884's test that a definition in an expression
# position keeps its expansion's binding (on main, its 18 other tests making
# 21790 as before and the new test 147, so 21937 at 16 and 6 without stress,
# the minimum left), merged beside the switch's tests: its 24 other tests
# making 22610 as at the switch's head and the new test 147, so 22757 at 16
# and 6 without stress, the minimum left; and macro_definition_env's test
# count re-pinned 2026-11-20 for the switch's review (#837), a library
# definer of its private name run twice where a definition is an
# expression, which keeps two bindings: its 53 other tests making 39531 as
# before and the new test 77, so 39608 at 16 and 127 without stress, the
# minimum left; and again that day for an expansion's own definition in a
# body reached before it runs, which leaves the library's binding as it
# was: its 54 other tests making 39608 and the new test 74, so 39682 at 16
# and 127 without stress, the minimum left; and relinked-names.scm's row
# for a definer's keyword beside a macro generated before it left
# scheme_suite's counts as they were, 10653 at 4096 and 385 without
# stress; and macro_definition_env's again that day for an ambiguous
# reference to a relinked identifier, named as its template wrote it: its
# 55 other tests making 39682 and the new test 70, so 39752 at 16 and 127
# without stress, the minimum left; and again that day for the switch's
# mutation stage: six tests (a macro a library makes expanded in its later
# forms, a generator's name spelled as a relinked one, a body scan that
# puts back a generated macro's carried keywords, a template's quote that
# is its library's own macro, two expansions' relinks of one spelling in
# one generated macro, a splice an expansion wrote), which collect 1241 at
# 16, beside its 56 other tests, two of them extended, making 39829, so
# 41070 at 16 and 129 without stress, the minimum left; and
# vm_callprimitive's count at 16 rose from 1893 to 1899 for the
# generated-macro case its re-pointed private binding test gained, 0
# without stress, its test count and minimum left; and scheme_suite's
# count without stress re-measured 2026-11-21 for #837's second PR's last
# part, which took the macro-expansion alias table and its latch out of
# every environment, so that a tree-walker closure is charged 572 bytes for
# the frame it captures where it was 612 (CAPTURED_FRAME_BYTES): the byte
# trigger comes later, 368 collections without stress where there were 385
# (385 again with that estimate alone put back at 612), and 10653 at 4096
# as before, the minimum left; every other target read as it did, at its
# interval and without stress; and raise_site_matrix pinned 2026-11-22
# with #901's raise-site tests, 59546 collections at 16 and 55710 without
# stress, nearly all of them its zeal interpreters', which collect at every
# outermost safe point whatever the lane's variable says, so its minimum
# is midway between the two; and re-pinned that day for #714's position
# axis, which calls each site from a tail and a non-tail position and so
# doubles its rows, and adds a test that the VM compiles each row's call as
# its position names: 119089 collections at 16 and 111441 without stress,
# its minimum midway again, and 7 tests)
TARGETS=(
    "callability 16 31 13"                    # 2131 (0)
    "control_flow_matrix 16 340 3"            # 4698 (0)
    "cps_features 16 530 3"                   # 1009 (2)
    "ephemerons 16 289 30"                    # 5738 (204)
    "escape_from_primitive 16 3669 19"        # 9789 (94)
    "finished_forms_release_code 16 2040 12"  # 4057 (15)
    "gc_tree_walker 16 112579 41"             # 129201 (95958)
    "gc_vm 16 44645 54"                       # 58197 (31094)
    "host_environments 16 83806 36"           # 167847 (765)
    "hygiene_matrix 16 11136 25"              # 22757 (6)
    "interpreter_api 16 138 28"               # 3050 (4)
    "library_loading 16 46 9"                 # 129 (0): see the header
    "macro_definition_env 16 16626 62"        # 41070 (129): see the header
    "owned_handles 16 62714 9"                # 125648 (14)
    "raise_site_matrix 16 115265 7"           # 119089 (111441)
    "rooted_loading 16 371 6"                 # 1269 (10)
    "vm_callprimitive 16 545 23"              # 1899 (0)
    "scheme_suite 4096 1534 11"               # 10653 (368): see the header
)

OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

fail=0
lane_start=$SECONDS
for entry in "${TARGETS[@]}"; do
    read -r target interval minimum tests <<< "$entry"
    run="$OUT/$target"
    mkdir -p "$run/count"
    start=$SECONDS
    rc=0
    PATINA_GC_STRESS=$interval PATINA_GC_COUNT_DIR="$run/count" \
        cargo test --workspace --test "$target" "$@" > "$run/output.txt" 2>&1 || rc=$?
    took=$((SECONDS - start))
    problems=()

    if [ "$rc" -ne 0 ]; then
        problems+=("cargo test exited $rc")
    fi
    result=$(grep -E '^test result: ' "$run/output.txt" | tail -1 || true)
    if [ -z "$result" ]; then
        problems+=("no test result line")
    else
        passed=$(sed -n 's/^test result: [A-Za-z]*\. \([0-9]*\) passed;.*/\1/p' <<< "$result")
        filtered=$(sed -n 's/.*; \([0-9]*\) filtered out.*/\1/p' <<< "$result")
        if [ "${filtered:-x}" != 0 ]; then
            problems+=("${filtered:-an unknown number of} test(s) filtered out")
        fi
        if [ "${passed:-0}" -lt "$tests" ]; then
            problems+=("${passed:-no} test(s) passed, pinned $tests")
        fi
    fi

    # The test binary's own record, and any from processes it started; each
    # must have run under this interval.
    collections=0
    own=0
    for record in "$run"/count/gc-count.*; do
        [ -e "$record" ] || continue
        case "$(cat "$record")" in
            *" exe=$target-"*) own=1 ;;
        esac
        if ! grep -q " env=PATINA_GC_STRESS=$interval " "$record"; then
            problems+=("a process ran under another mode: $(cat "$record")")
        fi
        n=$(sed -n 's/.* collections=0*\([0-9][0-9]*\)$/\1/p' "$record")
        collections=$((collections + ${n:-0}))
    done
    if [ "$own" -eq 0 ]; then
        problems+=("no collection record from the test binary: PATINA_GC_COUNT_DIR never reached it")
    fi
    if [ "$collections" -lt "$minimum" ]; then
        problems+=("$collections collections, pinned minimum $minimum")
    fi

    line="$target at $interval: ${result#test result: }, $collections collections (minimum $minimum), ${took}s"
    if [ ${#problems[@]} -eq 0 ]; then
        echo "OK   $line"
    else
        echo "FAIL $line"
        for p in "${problems[@]}"; do echo "       $p"; done
        echo "       output (failures and panics):"
        grep -E 'panicked at|^test .* FAILED$|^failures:|^error' "$run/output.txt" | head -20 \
            | sed 's/^/         /' || true
        fail=1
    fi
done

echo ""
echo "GC stress lane: $((SECONDS - lane_start))s"
exit $fail
