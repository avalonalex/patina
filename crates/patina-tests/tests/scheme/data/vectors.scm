;; Vectors — R7RS §6.8.
;;
;; Migrated from `crates/patina-tests/tests/compliance/vectors.rs` (#193),
;; which is deleted. One row here per Rust assertion, less the two that were
;; the same expression as another row: the Rust file's self-evaluation test
;; repeated `#(1 2 3)` and `#(a b c)` from its literal test, and each appears
;; once below.
;;
;; Programs that defined a vector and then mutated it are `let` bodies. Every
;; mutated vector is made by `vector`, `make-vector` or `vector-copy`; a
;; vector literal is a constant, and mutating one "is an error" (§3.4).

(import (scheme base) (srfi 64))

(test-begin "vectors")

;; ─── Literals and predicates ─────────────────────────────────────────────────

;; Vector literals are self-evaluating: none of these is quoted.
(test-equal "the empty vector literal" #() #())
(test-equal "a vector literal of numbers" '#(1 2 3) #(1 2 3))
(test-equal "a vector literal of symbols" '#(a b c) #(a b c))
(test-equal "a vector literal of mixed data" '#(0 (2 2 2 2) "Anna")
  #(0 (2 2 2 2) "Anna"))

(test-equal "vector? of the empty vector" #t (vector? #()))
(test-equal "vector? of a vector" #t (vector? #(1 2 3)))
(test-equal "vector? of a quoted vector" #t (vector? '#(1 2 3)))
(test-equal "vector? of the empty list" #f (vector? '()))
(test-equal "vector? of a number" #f (vector? 42))
(test-equal "vector? of a string" #f (vector? "string"))

;; ─── Construction and length ─────────────────────────────────────────────────

(test-equal "make-vector of length 0" 0 (vector-length (make-vector 0)))
(test-equal "make-vector of length 5" 5 (vector-length (make-vector 5)))
(test-equal "make-vector with a fill" #(a a a) (make-vector 3 'a))
(test-equal "make-vector of length 0 with a fill" #() (make-vector 0 'x))

(test-equal "vector of nothing" #() (vector))
(test-equal "vector of symbols" #(a b c) (vector 'a 'b 'c))
(test-equal "vector of numbers" #(1 2 3 4 5) (vector 1 2 3 4 5))

(test-equal "vector-length of the empty vector" 0 (vector-length #()))
(test-equal "vector-length of three" 3 (vector-length #(a b c)))
(test-equal "vector-length of a large vector" 1000
  (vector-length (make-vector 1000)))

;; ─── vector-ref and vector-set! ──────────────────────────────────────────────

(test-equal "vector-ref in the middle" 8 (vector-ref '#(1 1 2 3 5 8 13 21) 5))
(test-equal "vector-ref at 0" 1 (vector-ref '#(1 1 2 3 5 8 13 21) 0))
(test-equal "vector-ref at the end" 21 (vector-ref '#(1 1 2 3 5 8 13 21) 7))
(test-equal "vector-ref of a symbol" 'b (vector-ref '#(a b c) 1))

(test-error "vector-ref at the length" #t (vector-ref '#(1 2 3) 3))
(test-error "vector-ref at a negative index" #t (vector-ref '#(1 2 3) -1))
(test-error "vector-ref of a non-vector" #t (vector-ref '() 0))

(test-equal "vector-set! replaces an element" '#(0 ("Sue" "Sue") "Anna")
  (let ((vec (vector 0 '(2 2 2 2) "Anna")))
    (vector-set! vec 1 '("Sue" "Sue"))
    vec))
(test-equal "vector-set! at every index" #(10 20 30)
  (let ((v (make-vector 3 0)))
    (vector-set! v 0 10)
    (vector-set! v 1 20)
    (vector-set! v 2 30)
    v))

;; ─── Conversions ─────────────────────────────────────────────────────────────

(test-equal "vector->list of the empty vector" '() (vector->list '#()))
(test-equal "vector->list" '(dah dah didah) (vector->list '#(dah dah didah)))
(test-equal "vector->list from a start" '(dah didah)
  (vector->list '#(dah dah didah) 1))
(test-equal "vector->list of a range" '(dah) (vector->list '#(dah dah didah) 1 2))
(test-equal "vector->list of the whole range" '(a b c d)
  (vector->list '#(a b c d) 0 4))
(test-equal "vector->list of a trailing range" '(c d)
  (vector->list '#(a b c d) 2 4))

(test-equal "list->vector of the empty list" #() (list->vector '()))
(test-equal "list->vector of symbols" #(dididit dah) (list->vector '(dididit dah)))
(test-equal "list->vector of numbers" #(1 2 3 4 5) (list->vector '(1 2 3 4 5)))

(test-equal "string->vector of the empty string" #() (string->vector ""))
(test-equal "string->vector" #(#\A #\B #\C) (string->vector "ABC"))
(test-equal "string->vector from a start" #(#\B #\C) (string->vector "ABC" 1))
(test-equal "string->vector of a range" #(#\B) (string->vector "ABC" 1 2))

(test-equal "vector->string of the empty vector" "" (vector->string #()))
(test-equal "vector->string" "123" (vector->string #(#\1 #\2 #\3)))
(test-equal "vector->string from a start" "bcd"
  (vector->string #(#\a #\b #\c #\d) 1))
(test-equal "vector->string of a range" "bc"
  (vector->string #(#\a #\b #\c #\d) 1 3))

(test-equal "list->vector round-trips" #(a b c) (list->vector (vector->list #(a b c))))
(test-equal "vector->list round-trips" '(1 2 3) (vector->list (list->vector '(1 2 3))))
(test-equal "string->vector round-trips" #(#\x #\y #\z)
  (string->vector (vector->string #(#\x #\y #\z))))

;; ─── Copying ─────────────────────────────────────────────────────────────────

(test-equal "vector-copy of the empty vector" #() (vector-copy #()))
(test-equal "vector-copy" #(a b c) (vector-copy #(a b c)))
(test-equal "vector-copy from a start" #(b c) (vector-copy #(a b c) 1))
(test-equal "vector-copy of a range" #(b) (vector-copy #(a b c) 1 2))
;; The copy is a new vector: mutating it leaves the original alone.
(test-equal "vector-copy makes a new vector" '(#(1 8 2 8) #(3 8 2 8))
  (let* ((a #(1 8 2 8))
         (b (vector-copy a)))
    (vector-set! b 0 3)
    (list a b)))

(test-equal "vector-copy! into another vector" #(10 1 2 40 50)
  (let ((a (vector 1 2 3 4 5))
        (b (vector 10 20 30 40 50)))
    (vector-copy! b 1 a 0 2)
    b))
;; R7RS requires an overlapping copy to behave as if through a temporary.
(test-equal "vector-copy! within one vector" #(3 4 5 4 5)
  (let ((v (vector 1 2 3 4 5)))
    (vector-copy! v 0 v 2 5)
    v))

(test-equal "vector-append of nothing" #() (vector-append))
(test-equal "vector-append of the empty vector" #() (vector-append #()))
(test-equal "vector-append of two empty vectors" #() (vector-append #() #()))
(test-equal "vector-append onto the empty vector" #(a b c) (vector-append #() #(a b c)))
(test-equal "vector-append of the empty vector at the end" #(a b c)
  (vector-append #(a b c) #()))
(test-equal "vector-append of two" #(a b c d e) (vector-append #(a b c) #(d e)))
(test-equal "vector-append of three" #(1 2 3 4 5 6) (vector-append #(1 2) #(3 4) #(5 6)))

;; ─── vector-fill! ────────────────────────────────────────────────────────────

(test-equal "vector-fill! of a range" #(1 2 smash smash 5)
  (let ((a (vector 1 2 3 4 5)))
    (vector-fill! a 'smash 2 4)
    a))
(test-equal "vector-fill! of the whole vector" #(x x x x x)
  (let ((v (make-vector 5 0)))
    (vector-fill! v 'x)
    v))
(test-equal "vector-fill! of another range" #(1 99 99 4 5)
  (let ((v (vector 1 2 3 4 5)))
    (vector-fill! v 99 1 3)
    v))

;; ─── vector-map and vector-for-each ──────────────────────────────────────────

(test-equal "vector-map over one vector" #(1 4 9 16)
  (vector-map (lambda (x) (* x x)) #(1 2 3 4)))
(test-equal "vector-map over two vectors" #(5 7 9) (vector-map + #(1 2 3) #(4 5 6)))
;; The shortest vector determines the length.
(test-equal "vector-map stops at the shortest vector" #(5 7 9)
  (vector-map + #(1 2 3) #(4 5 6 7)))
(test-equal "vector-map over empty vectors" #() (vector-map + #() #()))

(test-equal "vector-for-each for its effects" 10
  (let ((sum 0))
    (vector-for-each (lambda (x) (set! sum (+ sum x))) #(1 2 3 4))
    sum))
;; vector-for-each goes in order, from index 0 — R7RS §6.8 guarantees it,
;; unlike vector-map — so the reversed accumulation is deterministic.
(test-equal "vector-for-each over two vectors, in order" '(11 22 33)
  (let ((result '()))
    (vector-for-each (lambda (x y) (set! result (cons (+ x y) result)))
                     #(1 2 3) #(10 20 30))
    (reverse result)))

;; ─── equal? on vectors ───────────────────────────────────────────────────────

(test-equal "equal? of empty vectors" #t (equal? #() #()))
(test-equal "equal? of equal vectors of numbers" #t (equal? #(1 2 3) #(1 2 3)))
(test-equal "equal? of equal vectors of symbols" #t (equal? #(a b c) #(a b c)))
(test-equal "equal? of vectors differing in an element" #f (equal? #(1 2 3) #(1 2 4)))
(test-equal "equal? of vectors differing in length" #f (equal? #(1 2) #(1 2 3)))
(test-equal "equal? of nested vectors" #t (equal? #(#(1 2) #(3 4)) #(#(1 2) #(3 4))))
(test-equal "equal? of vectors of lists" #t (equal? #((a b) (c d)) #((a b) (c d))))

(test-end)
