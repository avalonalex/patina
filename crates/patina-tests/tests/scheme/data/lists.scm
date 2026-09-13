;; Pairs and lists — R7RS §6.4.
;;
;; Migrated from `crates/patina-tests/tests/compliance/lists.rs` (#193), which
;; is deleted. Its tests were based on chibi-scheme's r7rs-tests.scm; one row
;; here per Rust assertion. The Rust tests that were whole programs, a
;; `define` followed by a mutation, are `let` bodies here, so one file can
;; hold every one of them without redefining a top-level name.

(import (scheme base) (scheme char) (srfi 64))

(test-begin "lists")

;; ─── Constructors and accessors ──────────────────────────────────────────────

(test-equal "cons of two numbers is a pair" '(1 . 2) (cons 1 2))
(test-equal "cons of two symbols is a pair" '(a . b) (cons 'a 'b))
(test-equal "cons onto the empty list" '(1) (cons 1 '()))
(test-equal "nested cons builds a list" '(1 2) (cons 1 (cons 2 '())))

(test-equal "car of a pair" 1 (car (cons 1 2)))
(test-equal "car of a list" 'a (car '(a b c)))
(test-equal "cdr of a pair" 2 (cdr (cons 1 2)))
(test-equal "cdr of a list" '(b c) (cdr '(a b c)))

;; ─── Predicates ──────────────────────────────────────────────────────────────

(test-equal "null? of the empty list" #t (null? '()))
(test-equal "null? of a list" #f (null? '(a)))
(test-equal "null? of a number" #f (null? 42))

(test-equal "pair? of a pair" #t (pair? (cons 1 2)))
(test-equal "pair? of a list" #t (pair? '(a b c)))
(test-equal "pair? of the empty list" #f (pair? '()))
(test-equal "pair? of a number" #f (pair? 42))

;; ─── List operations ─────────────────────────────────────────────────────────

(test-equal "list of nothing" '() (list))
(test-equal "list of one" '(1) (list 1))
(test-equal "list of three" '(1 2 3) (list 1 2 3))

(test-equal "list? of the empty list" #t (list? '()))
(test-equal "list? of a list" #t (list? '(1 2 3)))
(test-equal "list? of a dotted pair" #f (list? '(1 . 2)))
(test-equal "list? of a number" #f (list? 42))

(test-equal "length of the empty list" 0 (length '()))
(test-equal "length of one" 1 (length '(a)))
(test-equal "length of three" 3 (length '(a b c)))

(test-equal "append two lists" '(a b c) (append '(a) '(b c)))
(test-equal "append onto the empty list" '(a b) (append '() '(a b)))
(test-equal "append the empty list" '(a b) (append '(a b) '()))
(test-equal "append three lists" '(a b c d e f) (append '(a b) '(c d) '(e f)))

(test-equal "reverse the empty list" '() (reverse '()))
(test-equal "reverse one" '(a) (reverse '(a)))
(test-equal "reverse three" '(c b a) (reverse '(a b c)))

(test-equal "list-ref at 0" 'a (list-ref '(a b c d) 0))
(test-equal "list-ref at 2" 'c (list-ref '(a b c d) 2))
(test-equal "list-tail at 0" '(a b c d) (list-tail '(a b c d) 0))
(test-equal "list-tail at 2" '(c d) (list-tail '(a b c d) 2))

;; ─── The cxr combinations in (scheme base) ───────────────────────────────────

(test-equal "caar" 'a (caar '((a b) c d)))
(test-equal "cadr" 'b (cadr '(a b c d)))
(test-equal "cdar" '(b) (cdar '((a b) c d)))
(test-equal "cddr" '(c d) (cddr '(a b c d)))

;; ─── Membership ──────────────────────────────────────────────────────────────

(test-equal "memq finds a symbol" '(b c) (memq 'b '(a b c)))
(test-equal "memq misses" #f (memq 'd '(a b c)))
(test-equal "memq finds the first element" '(a b c) (memq 'a '(a b c)))
(test-equal "memv finds a number" '(101 102) (memv 101 '(100 101 102)))
(test-equal "memv misses" #f (memv 99 '(100 101 102)))
;; member uses equal?, so it can find lists.
(test-equal "member finds a list" '((a) c) (member (list 'a) '(b (a) c)))
(test-equal "member misses" #f (member 'a '(b c d)))
;; R7RS: member takes an optional comparison procedure.
(test-equal "member with = finds" '(2 3) (member 2 '(1 2 3) =))
(test-equal "member with = misses" #f (member 5 '(1 2 3) =))
(test-equal "member with string-ci=?" '("b" "c")
  (member "B" '("a" "b" "c") string-ci=?))

;; ─── Association lists ───────────────────────────────────────────────────────

(test-equal "assq finds" '(b 2) (assq 'b '((a 1) (b 2) (c 3))))
(test-equal "assq misses" #f (assq 'd '((a 1) (b 2) (c 3))))
(test-equal "assv finds" '(5 7) (assv 5 '((2 3) (5 7) (11 13))))
(test-equal "assv misses" #f (assv 6 '((2 3) (5 7) (11 13))))
;; assoc uses equal?, so it can find lists as keys.
(test-equal "assoc finds a list key" '((a)) (assoc (list 'a) '(((a)) ((b)) ((c)))))
(test-equal "assoc finds a symbol key" '(b) (assoc 'b '((a) (b) (c))))
(test-equal "assoc misses" #f (assoc 'd '((a) (b) (c))))
;; R7RS: assoc takes an optional comparison procedure.
(test-equal "assoc with = finds" '(2 b) (assoc 2 '((1 a) (2 b) (3 c)) =))
(test-equal "assoc with = misses" #f (assoc 5 '((1 a) (2 b) (3 c)) =))
(test-equal "assoc with string-ci=?" '("b" 2)
  (assoc "B" '(("a" 1) ("b" 2) ("c" 3)) string-ci=?))

;; ─── Higher-order operations ─────────────────────────────────────────────────

(test-equal "map over one list" '(2 4 6) (map (lambda (x) (* x 2)) '(1 2 3)))
(test-equal "map a primitive" '(1 3 5) (map car '((1 2) (3 4) (5 6))))
(test-equal "map over two lists" '(5 7 9) (map + '(1 2 3) '(4 5 6)))
(test-equal "map stops at the shortest list" '(5 7 9) (map + '(1 2 3) '(4 5 6 7)))
(test-equal "map over the empty list" '() (map (lambda (x) (* x 2)) '()))

(test-equal "for-each for its effects" 10
  (let ((result 0))
    (for-each (lambda (x) (set! result (+ result x))) '(1 2 3 4))
    result))
(test-equal "for-each over two lists" 21
  (let ((sum 0))
    (for-each (lambda (x y) (set! sum (+ sum x y))) '(1 2 3) '(4 5 6))
    sum))

;; ─── Mutation ────────────────────────────────────────────────────────────────

(test-equal "set-car! replaces the car" 10
  (let ((p (cons 1 2))) (set-car! p 10) (car p)))
(test-equal "set-cdr! replaces the cdr" 20
  (let ((p (cons 1 2))) (set-cdr! p 20) (cdr p)))
(test-equal "set-car! on a list" '(x b c)
  (let ((lst (list 'a 'b 'c))) (set-car! lst 'x) lst))
;; set-cdr! on a list replaces the whole tail.
(test-equal "set-cdr! on a list" '(a y z)
  (let ((lst (list 'a 'b 'c))) (set-cdr! lst '(y z)) lst))

(test-equal "list-set! in the middle" '(0 1 x 3 4)
  (let ((lst (list 0 1 2 3 4))) (list-set! lst 2 'x) lst))
(test-equal "list-set! at index 0" '(x b c)
  (let ((lst (list 'a 'b 'c))) (list-set! lst 0 'x) lst))
(test-equal "list-set! at the last index" '(a b z)
  (let ((lst (list 'a 'b 'c))) (list-set! lst 2 'z) lst))

;; A mutation is visible through every reference to the pair.
(test-equal "mutation is visible through a shared reference" 'z
  (let* ((x (list 'a 'b 'c)) (y x)) (set-car! x 'z) (car y)))
;; eqv? compares pairs by identity, so mutating one leaves it eqv? to itself.
(test-equal "eqv? of a pair and itself after mutation" #t
  (let* ((x (list 'a 'b 'c)) (y x)) (set-cdr! x 4) (eqv? x y)))
;; list? must detect a circular list built with set-cdr!, not loop on it.
(test-equal "list? of a circular list" #f
  (let ((x (list 'a))) (set-cdr! x x) (list? x)))

;; #275: proper-list consumers reject circular cdr chains with catchable
;; errors. A cycle through a car is an ordinary element, not a circular list.
;; These are R7RS "is an error" inputs; signalling is Patina's choice.
;; Chibi 0.12 and Gauche 0.9.15 timed out in bounded subprocess probes for
;; append, reverse, and list->string (2026-09-12). Skip only those calls so
;; their length/list->vector checks and the valid circular-tail cases run.
(define self-cycle (list #\a))
(set-cdr! self-cycle self-cycle)
(define prefixed-cycle (list #\a #\b #\c #\d))
(set-cdr! (cdr (cddr prefixed-cycle)) (cdr prefixed-cycle))

(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "append rejects a self-cycle" #t
  (append self-cycle '(tail)))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "append rejects a cycle after a prefix" #t
  (append prefixed-cycle '(tail)))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "reverse rejects a self-cycle" #t
  (reverse self-cycle))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "reverse rejects a cycle after a prefix" #t
  (reverse prefixed-cycle))
(test-error "list->vector rejects a self-cycle" #t
  (list->vector self-cycle))
(test-error "list->vector rejects a cycle after a prefix" #t
  (list->vector prefixed-cycle))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "list->string rejects a self-cycle" #t
  (list->string self-cycle))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "list->string rejects a cycle after a prefix" #t
  (list->string prefixed-cycle))
(test-error "length rejects a self-cycle" #t
  (length self-cycle))
(test-error "length rejects a cycle after a prefix" #t
  (length prefixed-cycle))
(test-assert "append shares a circular final argument"
  (let ((result (append '(1 2) prefixed-cycle)))
    (eq? (cddr result) prefixed-cycle)))
(test-assert "append returns a sole circular argument unchanged"
  (eq? (append self-cycle) self-cycle))
(test-equal "bounded access still works on a circular list" '(#\a #\c #t)
  (list (list-ref self-cycle 9)
        (list-ref prefixed-cycle 8)
        (eq? (list-tail prefixed-cycle 7) (cdr prefixed-cycle))))
(test-assert "a cycle through car remains a valid list element"
  (let ((x (list #f)))
    (set-car! x x)
    (and (eq? (vector-ref (list->vector x) 0) x)
         (eq? (car (reverse x)) x))))
(test-equal "append still accepts an improper final argument" '(1 2 . 3)
  (append '(1) '(2 . 3)))
;; Chibi/Gauche sometimes discard improper tails rather than signalling.
;; Those permitted differences are recorded as latitude in DIVERGENCES.tsv.
(test-error "append rejects an improper non-final argument" #t
  (append '(1 . 2) '(3)))
(test-error "reverse rejects an improper list" #t (reverse '(1 . 2)))
(test-error "list->vector rejects an improper list" #t (list->vector '(1 . 2)))
(test-error "list->string rejects an improper list" #t (list->string '(#\a . 2)))

(test-end)
