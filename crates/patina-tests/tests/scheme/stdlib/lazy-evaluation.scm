;; `(scheme lazy)` — `delay`, `delay-force`, `force`, `promise?`,
;; `make-promise`. R7RS §4.2.5 and §6.10.
;;
;; Migrated whole from `crates/patina-tests/tests/lazy_evaluation.rs` (#193
;; Phase 1). Every row there was one `assert_program_eval_to`, so nothing stayed
;; behind: 26 rows in, 26 out, plus **three added** where review found a
;; documented guarantee that no row checked — `make-promise`'s identity,
;; `(force (delay p))` yielding `p`, and `delay-force` over a long chain.
;;
;; **Names are unique per row.** In the `.rs` form each row was its own program
;; with its own top level, and thirteen of them defined `p`. Here they share one, so
;; a later `define` would replace an earlier row's promise — and for the rows
;; that count evaluations, sharing a counter would not fail loudly, it would
;; just stop measuring what the row claims to measure.
;;
;; The one binding deliberately shared is `lazy-range`, which three rows defined
;; identically. It is the shape under test in all three, not per-row state.
;;
;; Two rows fail under chibi by design — see the note on `force` of a
;; non-promise. Gauche runs all 29.
;;
;; Some rows here are the same claim twice: the "R7RS examples" section repeats
;; what the sections above it establish. They are kept and marked rather than
;; deduplicated, because a row that quotes the report is evidence about the
;; report and not only about the implementation.

(import (scheme base) (scheme lazy) (srfi 64))

(test-begin "lazy-evaluation")

;; ── delay and force ─────────────────────────────────────────────────────────

(define p-sum (delay (+ 1 2)))
(test-equal "force evaluates a delayed expression" 3 (force p-sum))

;; The delayed expression must not run when the promise is *built*. That is all
;; this row shows: the assertion runs immediately after the `define`, so it says
;; nothing about what happens later, and `p-unforced` is deliberately never
;; forced. The row below is the other half — that forcing is what runs it.
(define lazily-set 10)
(define p-unforced (delay (set! lazily-set 20)))
(test-equal "delay does not evaluate its expression" 10 lazily-set)

(define eagerly-set 10)
(define p-forced (delay (set! eagerly-set 20)))
(test-equal "…and force is what makes it happen" 20
  (begin (force p-forced) eagerly-set))

(test-equal "a delayed expression may be arbitrarily complex" 230
  (force (delay (let ((x 10) (y 20)) (+ x y (* x y))))))

;; ── Memoisation ─────────────────────────────────────────────────────────────

;; R7RS §4.2.5: a promise's expression is evaluated at most once, and the value
;; is retained.
;;
;; The first two rows are the same claim twice — in the `.rs` file they were
;; `test_force_caches_result` and `test_force_doesnt_re_evaluate`, differing
;; only in whether the promise returns the counter or a constant. Kept both, and
;; said so, rather than quietly migrating 26 rows as 25: a count that matches
;; the file it replaced is one of the few automatic checks this migration has.
(define force-count 0)
(define p-counted (delay (begin (set! force-count (+ force-count 1)) force-count)))
(test-equal "forcing three times evaluates once" 1
  (begin (force p-counted) (force p-counted) (force p-counted) force-count))

(define eval-count 0)
(define p-eval-counted (delay (begin (set! eval-count (+ eval-count 1)) 42)))
(test-equal "…and the promise's own value does not change that" 1
  (begin (force p-eval-counted) (force p-eval-counted) (force p-eval-counted)
         eval-count))

;; A different claim from the two above, and it was nearly lost: the `.rs`
;; row (`test_force_evaluates_thunk`) asserted what `force` *returns* — the
;; value the thunk produced — not what a counter reads afterwards. Migrating it
;; as a counter check turned a distinct row into a third copy of memoisation,
;; and the count still said 26, which is how the check that catches dropped rows
;; fails to catch replaced ones.
(define increment-count 0)
(define (increment) (set! increment-count (+ increment-count 1)) increment-count)
(define p-increment (delay (increment)))
(test-equal "force returns the value the thunk produced" 1 (force p-increment))

;; ── force on a non-promise ──────────────────────────────────────────────────

;; **This is an extension, not a guarantee of the report, and it is where the
;; oracles part company.** R7RS §6.10 defines `force` only on a promise created
;; by `delay`, `delay-force` or `make-promise`, and the R7RS test suite never
;; forces anything else. Patina returns the object unchanged — deliberately,
;; and its primitive says so (`patina-primitives/src/primitives/lazy.rs`) — and
;; Gauche agrees. **chibi raises instead**, so this row and its R7RS-section
;; twin below are the two chibi reports as failures (27 of 29 there; Gauche is
;; 29 of 29).
;;
;; Kept as ordinary rows rather than hidden behind `cond-expand`: what they pin
;; is a real guarantee Patina makes, and a `cond-expand (patina)` would also
;; drop Gauche's corroboration, which is the more useful half. Anyone using
;; chibi on this file should expect exactly these two.
(test-equal "forcing a non-promise returns it" 42 (force 42))

;; ── promise? ────────────────────────────────────────────────────────────────

(define p-predicate (delay 1))
(test-equal "a promise answers promise?" #t (promise? p-predicate))
(test-equal "an ordinary value does not" #f (promise? 42))

;; A promise stays a promise after it has been forced — forcing yields the
;; value, it does not replace the object.
(define p-still (delay 1))
(test-equal "a forced promise is still a promise" #t
  (begin (force p-still) (promise? p-still)))

;; ── make-promise ────────────────────────────────────────────────────────────

(define p-made (make-promise 42))
(test-equal "make-promise wraps a value" 42 (force p-made))

;; R7RS §6.10: `make-promise` on a promise returns **that** promise. `promise?`
;; alone would not say so — an implementation that wrapped it in a fresh promise
;; would pass — so the row asks for identity. `eq?` here is portable: all four
;; implementations answer #t.
(define p-inner (delay 1))
(define p-rewrapped (make-promise p-inner))
(test-equal "make-promise on a promise yields a promise" #t (promise? p-rewrapped))
(test-equal "…and it is the same promise, not a fresh wrapper" #t
  (eq? p-rewrapped p-inner))

;; ── delay-force ─────────────────────────────────────────────────────────────

;; §4.2.5's purpose for `delay-force`: the promise it builds, when forced,
;; forces the promise its body produces — *without* growing the recursion, which
;; is the whole reason it exists rather than `(delay (force …))`.
(define p-df-once (delay-force (delay 42)))
(test-equal "delay-force forces the promise its body returns" 42 (force p-df-once))

(define p-df-twice (delay-force (delay-force (delay 42))))
(test-equal "…and does so through a chain of them" 42 (force p-df-twice))

;; The space property, which the two rows above do *not* show — two links prove
;; nothing about a long chain. Unlike `tail-recursion.scm`'s constant-space
;; claim, this one is within reach of an ordinary `test-equal`: a chain that
;; grew the recursion would exhaust the stack well before 200 000, which
;; `lazy.rs` records as a real past failure ("a chain of a hundred thousand …
;; overflowed the stack"). Portable — all four implementations return `done`.
(define (df-chain n) (if (= n 0) (delay 'done) (delay-force (df-chain (- n 1)))))
(test-equal "a 200 000-link delay-force chain does not grow the recursion" 'done
  (force (df-chain 200000)))

;; What `delay` must *not* do: force through a promise its body evaluates to.
;; `(force (delay p))` is `p` itself, and `delay-force` is how you ask for the
;; other behaviour. This is what `%make-forced-promise` exists for
;; (`patina-primitives/src/primitives/lazy.rs`), and without this row `delay`
;; could be built on `make-promise` — which returns an existing promise
;; unchanged — with every other row in this file still passing.
(define p-inner-value (delay 1))
(test-equal "force of a delay whose body is a promise yields that promise" #t
  (eq? (force (delay p-inner-value)) p-inner-value))

;; Names read outside-in: `p-outer` is the one forced, and its body forces
;; `p-inner`.
(define p-inner-sum (delay (+ 1 2)))
(define p-outer-forcing (delay (force p-inner-sum)))
(test-equal "a promise whose body forces another promise" 3 (force p-outer-forcing))

;; ── Lazy data structures ────────────────────────────────────────────────────

;; Shared on purpose: all three rows below are about this one shape, and the
;; `.rs` file repeated it verbatim in each.
(define (lazy-range n)
  (delay (if (= n 0) '() (cons n (lazy-range (- n 1))))))

(test-equal "the head of a lazy list is an ordinary value" 5
  (car (force (lazy-range 5))))

(test-equal "its tail is still a promise" #t
  (promise? (cdr (force (lazy-range 5)))))

(test-equal "forcing the tail yields the next element" 2
  (car (force (cdr (force (lazy-range 3))))))

;; A lazy infinite sequence: each cell holds a value and a promise for the rest,
;; so nothing beyond what is forced is ever built.
(define (fib-gen a b) (delay (cons a (fib-gen b (+ a b)))))
(test-equal "three elements of an infinite lazy sequence" '(0 1 1)
  (let* ((fibs (fib-gen 0 1))
         (fib0 (car (force fibs)))
         (rest1 (cdr (force fibs)))
         (fib1 (car (force rest1)))
         (rest2 (cdr (force rest1)))
         (fib2 (car (force rest2))))
    (list fib0 fib1 fib2)))

;; A lazy filter. When the head fails the predicate the promise's value *is* the
;; rest of the filter, and this one reaches it with `force` in tail position —
;; which is exactly the shape `delay-force` exists to replace. Over a six-element
;; list it recurses once and nothing shows; over a long list it would grow the
;; stack. Kept as the `.rs` file wrote it, and flagged so it is not copied: the
;; `delay-force` row above is the version that scales.
(define (lazy-filter pred lst)
  (delay
    (if (null? lst)
        '()
        (let ((head (car lst))
              (tail (cdr lst)))
          (if (pred head)
              (cons head (lazy-filter pred tail))
              (force (lazy-filter pred tail)))))))
(test-equal "a lazy filter skips to its first match" 2
  (car (force (lazy-filter even? '(1 2 3 4 5 6)))))

;; Promises are first-class: they can be stored and retrieved like any value.
(define promise-list (list (delay 1) (delay 2) (delay 3)))
(test-equal "promises can be held in a data structure" 1
  (force (car promise-list)))

;; **Larceny family 3** (`scheme_tests/reports/larceny_triage.md`), moved here
;; from `larceny_families.rs`.
;;
;; The reason `delay-force` exists at all is that a chain of them runs in
;; bounded space — that is the whole of R7RS §7.3's argument for the form. Ours
;; recursed per link and overflowed at a hundred thousand until 2026-08-24,
;; when `force` became the report's iterative version with the inner promise
;; aliased to the outer's box.
;;
;; A regression here aborts the process rather than failing the row, so it is
;; ordered late in the file for the same reason `data/circular-data.scm`'s
;; stack rows are last.
(define (count-down n)
  (if (= n 0) (delay 'done) (delay-force (count-down (- n 1)))))
(test-equal "a long delay-force chain runs in bounded space" 'done
  (force (count-down 100000)))

;; ── The examples R7RS gives ─────────────────────────────────────────────────
;;
;; §6.10's own text, kept verbatim even where a row above already covers the
;; claim. These are the rows that say the *report* is satisfied, rather than
;; that the implementation is self-consistent.

(test-equal "R7RS: (force (delay (+ 1 2))) is 3" 3 (force (delay (+ 1 2))))

(define p-r7rs (delay (+ 1 2)))
(test-equal "R7RS: a delayed expression is a promise" #t (promise? p-r7rs))

(test-equal "R7RS: forcing a non-promise returns it" 5 (force 5))

(test-equal "R7RS: make-promise produces a promise" #t (promise? (make-promise 5)))

(test-end)
