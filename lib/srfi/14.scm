;;; SRFI 14: character sets, over the whole Unicode range.  -*- Scheme -*-
;;;
;;; Patina-authored. This replaces Olin Shivers' reference implementation,
;;; which stores a char-set as a 256-character string indexed by code point
;;; and whose own header says it "is Latin-1 specific. Would certainly have to
;;; be rewritten for Unicode." It would: on that representation a character
;;; above U+00FF indexes past the end of the string and raises, and
;;; `ucs-range->char-set` silently clips a range to the first 256 code points,
;;; so `char-set:full` had 256 members and a Hangul range was empty. See
;;; issue #372 and Larceny triage family 45.
;;;
;;; Representation: a char-set holds one list of inclusive code-point ranges,
;;;
;;;     ((lo . hi) (lo . hi) ...)
;;;
;;; kept ascending, disjoint and non-adjacent — `(0 . 5)` and `(6 . 9)` are
;;; always merged into `(0 . 9)`. That invariant is what makes the rest cheap:
;;; `char-set=` is `equal?` on the range lists, every set operation is a
;;; single merge down two ascending lists, and `char-set-size` sums range
;;; widths without visiting a member. `char-set:full` is one pair, so the
;;; universe costs the same as a singleton.
;;;
;;; Surrogates are not members of anything. They are not characters: Patina's
;;; `integer->char` rejects #xD800-#xDFFF, as Rust's `char` does, so no
;;; character can ask about one, and the ranges here skip that block. Every
;;; code point outside it from 0 to #x10FFFF is fair game, which is the
;;; 1112064 members `char-set:full` reports.
;;;
;;; The `char-set:*` class constants come from `char-set-unicode-ranges`, a
;;; Rust primitive in `(patina internal chars)`. Deriving them here would mean
;;; a predicate call per scalar value — 1.1M of them per class, measured at
;;; 0.1s on the VM and 1.8s on the tree-walker, times eleven classes, on every
;;; import. The primitive also gives the classes `(scheme char)`'s predicates
;;; are built from, so `char-set:letter` and `char-alphabetic?` cannot
;;; disagree; see that primitive's comment for the two Unicode sources and the
;;; test that holds them at one version.
;;;
;;; The `!` procedures are SRFI 14's "linear update": allowed but not required
;;; to reuse their first argument. They mutate the range-list field, so a
;;; char-set is a mutable box around an immutable list — the lists themselves
;;; are freely shared between sets, which is why `char-set-copy` need only
;;; allocate a new box.

;;; The universe
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

(define %code-point-limit #x110000)     ; one past the last code point
(define %surrogate-lo #xD800)
(define %surrogate-hi #xDFFF)

;;; Every code point that is a character, as a range list. `char-set:full`.
(define %universe
  (list (cons 0 (- %surrogate-lo 1))
        (cons (+ %surrogate-hi 1) (- %code-point-limit 1))))

;;; Drops the surrogate block out of an arbitrary inclusive range, yielding
;;; zero, one or two ranges. Every entry point that takes code points rather
;;; than characters goes through this, so the invariant "no range covers a
;;; surrogate" holds for the whole library.
(define (%clip-range lo hi)
  (cond ((> lo hi) '())
        ((< hi %surrogate-lo) (list (cons lo hi)))
        ((> lo %surrogate-hi) (list (cons lo hi)))
        (else
         (let ((below (if (< lo %surrogate-lo)
                          (list (cons lo (- %surrogate-lo 1)))
                          '()))
               (above (if (> hi %surrogate-hi)
                          (list (cons (+ %surrogate-hi 1) hi))
                          '())))
           (append below above)))))

;;; The record
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

(define-record-type <char-set>
  (%make-char-set ranges)
  char-set?
  (ranges %ranges %set-ranges!))

(define (%check-char-set cs proc)
  (if (char-set? cs)
      (%ranges cs)
      (error "Not a char-set" proc cs)))

(define (%check-char c proc)
  (if (char? c)
      c
      (error "Not a character" proc c)))

(define (%check-procedure p proc)
  (if (procedure? p)
      p
      (error "Not a procedure" proc p)))

;;; Range-list primitives
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;;; These are the whole implementation. Everything above them is bookkeeping.
;;; Each takes and returns ascending, disjoint, non-adjacent range lists.

;;; Union: interleave two ascending lists, then coalesce. The merge keeps the
;;; result ascending, and `%normalize` is what restores disjointness, so the
;;; two halves of the invariant are each one place.
(define (%range-union a b)
  (let loop ((a a) (b b) (acc '()))
    (cond
     ((null? a) (%normalize-onto acc b))
     ((null? b) (%normalize-onto acc a))
     (else
      (if (<= (caar a) (caar b))
          (loop (cdr a) b (cons (car a) acc))
          (loop a (cdr b) (cons (car b) acc)))))))

;;; Appends `rest` to the reverse of `acc`.
(define (%reverse-onto acc rest)
  (if (null? acc)
      rest
      (%reverse-onto (cdr acc) (cons (car acc) rest))))

;;; Reverses `acc` onto `rest` and re-normalizes, coalescing any ranges the
;;; merge left touching.
(define (%normalize-onto acc rest)
  (%normalize (%reverse-onto acc rest)))

;;; Coalesces an ascending but possibly touching/overlapping range list.
(define (%normalize ranges)
  (if (null? ranges)
      '()
      (let loop ((rs (cdr ranges))
                 (lo (caar ranges))
                 (hi (cdar ranges))
                 (acc '()))
        (cond
         ((null? rs) (reverse (cons (cons lo hi) acc)))
         ((<= (caar rs) (+ hi 1))
          (loop (cdr rs) lo (max hi (cdar rs)) acc))
         (else
          (loop (cdr rs) (caar rs) (cdar rs) (cons (cons lo hi) acc)))))))

;;; Sorts a range list ascending by start, then coalesces. Used where ranges
;;; arrive in no particular order (a list of characters, a fold).
(define (%ranges-from-sorted-pairs pairs)
  (%normalize (%sort-ranges pairs)))

(define (%sort-ranges ranges)
  ;; Merge sort: the inputs here are range lists up to ~850 long, and a
  ;; quadratic insertion sort on `char-set:graphic` is measurable.
  (define (merge a b)
    (cond ((null? a) b)
          ((null? b) a)
          ((<= (caar a) (caar b)) (cons (car a) (merge (cdr a) b)))
          (else (cons (car b) (merge a (cdr b))))))
  (define (split lst)
    (let loop ((slow lst) (fast (if (null? lst) '() (cdr lst))) (acc '()))
      (if (or (null? fast) (null? (cdr fast)))
          (values (reverse (cons (car slow) acc)) (cdr slow))
          (loop (cdr slow) (cddr fast) (cons (car slow) acc)))))
  (if (or (null? ranges) (null? (cdr ranges)))
      ranges
      (let-values (((left right) (split ranges)))
        (merge (%sort-ranges left) (%sort-ranges right)))))

;;; Complement within the universe.
(define (%range-complement ranges)
  (%range-difference %universe ranges))

;;; Intersection: walk both lists once, emitting the overlap of each pair.
(define (%range-intersection a b)
  (let loop ((a a) (b b) (acc '()))
    (if (or (null? a) (null? b))
        (reverse acc)
        (let ((alo (caar a)) (ahi (cdar a))
              (blo (caar b)) (bhi (cdar b)))
          (let ((lo (max alo blo)) (hi (min ahi bhi)))
            (let ((acc (if (<= lo hi) (cons (cons lo hi) acc) acc)))
              ;; Advance whichever range ends first; the other may still
              ;; overlap what follows it.
              (if (< ahi bhi)
                  (loop (cdr a) b acc)
                  (loop a (cdr b) acc))))))))

;;; Difference: the parts of `a` no range of `b` covers.
(define (%range-difference a b)
  (let loop ((a a) (b b) (acc '()))
    (cond
     ((null? a) (reverse acc))
     ((null? b) (%reverse-onto acc a))
     (else
      (let ((alo (caar a)) (ahi (cdar a))
            (blo (caar b)) (bhi (cdar b)))
        (cond
         ;; `b`'s range is entirely before `a`'s: discard it.
         ((< bhi alo) (loop a (cdr b) acc))
         ;; `a`'s range is entirely before `b`'s: it survives whole.
         ((< ahi blo) (loop (cdr a) b (cons (car a) acc)))
         (else
          ;; They overlap. Emit the part of `a` below `b`, then continue with
          ;; whatever of `a` lies above `b`.
          (let ((acc (if (< alo blo) (cons (cons alo (- blo 1)) acc) acc)))
            (if (> ahi bhi)
                (loop (cons (cons (+ bhi 1) ahi) (cdr a)) (cdr b) acc)
                (loop (cdr a) b acc))))))))))

(define (%range-contains? ranges cp)
  (let loop ((rs ranges))
    (and (pair? rs)
         (let ((lo (caar rs)) (hi (cdar rs)))
           (cond ((< cp lo) #f)            ; ascending, so no later range can
                 ((<= cp hi) #t)
                 (else (loop (cdr rs))))))))

(define (%range-size ranges)
  (let loop ((rs ranges) (n 0))
    (if (null? rs)
        n
        (loop (cdr rs) (+ n 1 (- (cdar rs) (caar rs)))))))

;;; `a` is a subset of `b`.
(define (%range-subset? a b)
  (null? (%range-difference a b)))

;;; Iteration
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

;;; Applies `proc` to every member, ascending by code point.
;;;
;;; SRFI 14 leaves the order of `char-set-fold`, `-for-each`, `->list` and the
;;; cursors unspecified, and the Latin-1 reference implementation this replaced
;;; walked descending. Ascending is chosen because both references walk that
;;; way — chibi and Gauche each accumulate `(#\h #\e #\c #\a #\T #\G)` from a
;;; set of G,a,T,e,c,h, which is an ascending walk consed into a list — and
;;; because Larceny's suite pins that accumulation. The range list is already
;;; ascending, so this is also the direction that needs no `reverse`.
(define (%range-for-each-ascending proc ranges)
  (let outer ((rs ranges))
    (if (pair? rs)
        (let ((hi (cdar rs)))
          (let inner ((cp (caar rs)))
            (if (<= cp hi)
                (begin (proc (integer->char cp))
                       (inner (+ cp 1)))))
          (outer (cdr rs))))))

(define (%range-fold-ascending kons knil ranges)
  (let outer ((rs ranges) (acc knil))
    (if (null? rs)
        acc
        (let ((hi (cdar rs)))
          (let inner ((cp (caar rs)) (acc acc))
            (if (> cp hi)
                (outer (cdr rs) acc)
                (inner (+ cp 1) (kons (integer->char cp) acc))))))))

;;; Constructors
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

;;; Parses the optional trailing BASE-CS argument the SRFI gives to
;;; `list->char-set`, `string->char-set`, `char-set-unfold`,
;;; `ucs-range->char-set` and `char-set-filter`. Returns its range list, or
;;; the empty set's when absent. The ranges are immutable and shared, so
;;; unlike the reference implementation this need not copy anything.
(define (%default-base maybe-base proc)
  (if (pair? maybe-base)
      (if (null? (cdr maybe-base))
          (%check-char-set (car maybe-base) proc)
          (error "Expected final base char set -- too many parameters"
                 proc maybe-base))
      '()))

(define (%chars->ranges chars proc)
  (%ranges-from-sorted-pairs
   (map (lambda (c)
          (let ((cp (char->integer (%check-char c proc))))
            (cons cp cp)))
        chars)))

(define (char-set . chars)
  (%make-char-set (%chars->ranges chars 'char-set)))

(define (list->char-set chars . maybe-base)
  (%make-char-set
   (%range-union (%default-base maybe-base 'list->char-set)
                 (%chars->ranges chars 'list->char-set))))

(define (list->char-set! chars base-cs)
  (%set-ranges! base-cs
                (%range-union (%check-char-set base-cs 'list->char-set!)
                              (%chars->ranges chars 'list->char-set!)))
  base-cs)

(define (string->char-set str . maybe-base)
  (%make-char-set
   (%range-union (%default-base maybe-base 'string->char-set)
                 (%chars->ranges (string->list str) 'string->char-set))))

(define (string->char-set! str base-cs)
  (%set-ranges! base-cs
                (%range-union (%check-char-set base-cs 'string->char-set!)
                              (%chars->ranges (string->list str)
                                              'string->char-set!)))
  base-cs)

(define (char-set-copy cs)
  ;; The range list is immutable, so the copy is a fresh box around the same
  ;; list; a later `!` on either replaces its own box's field.
  (%make-char-set (%check-char-set cs 'char-set-copy)))

(define (->char-set x)
  (cond ((char-set? x) x)
        ((string? x) (string->char-set x))
        ((char? x) (char-set x))
        (else (error "->char-set: Not a charset, string or char." x))))

;;; -- UCS ranges

;;; SRFI 14's `ucs-range->char-set` takes an *exclusive* upper bound, and
;;; signals when asked for characters the implementation does not have, if
;;; ERROR? is true. Every code point below #x110000 is available here except
;;; the surrogates, which are not characters in any implementation, so the
;;; only request that can be refused is one that runs off the end of Unicode.
(define (%ucs-range->ranges lower upper error? proc)
  (if (not (and (integer? lower) (exact? lower) (<= 0 lower)))
      (error "Invalid lower bound" proc lower))
  (if (not (and (integer? upper) (exact? upper) (<= lower upper)))
      (error "Invalid upper bound" proc upper))
  (if (and error? (> upper %code-point-limit))
      (error "Requested UCS range contains unavailable characters"
             proc lower upper))
  (%clip-range lower (- (min upper %code-point-limit) 1)))

(define (ucs-range->char-set lower upper . rest)
  ;; The base char-set follows ERROR?, so the rest list is `(error? base)`.
  (let ((error? (if (pair? rest) (car rest) #f))
        (base (%default-base (if (pair? rest) (cdr rest) '())
                             'ucs-range->char-set)))
    (%make-char-set
     (%range-union base
                   (%ucs-range->ranges lower upper error?
                                       'ucs-range->char-set)))))

(define (ucs-range->char-set! lower upper error? base-cs)
  (%set-ranges! base-cs
                (%range-union (%check-char-set base-cs 'ucs-range->char-set!)
                              (%ucs-range->ranges lower upper error?
                                                  'ucs-range->char-set!)))
  base-cs)

;;; -- Predicate -> char-set

;;; `char-set-filter` must call PRED on each member of DOMAIN, so it is the one
;;; place a per-character walk is unavoidable — the predicate is opaque.
(define (%filter-ranges pred domain-ranges proc)
  (%check-procedure pred proc)
  (%ranges-from-sorted-pairs
   (%range-fold-ascending (lambda (c acc)
                             (if (pred c)
                                 (let ((cp (char->integer c)))
                                   (cons (cons cp cp) acc))
                                 acc))
                           '()
                           domain-ranges)))

(define (char-set-filter pred domain . maybe-base)
  (%make-char-set
   (%range-union (%default-base maybe-base 'char-set-filter)
                 (%filter-ranges pred
                                 (%check-char-set domain 'char-set-filter)
                                 'char-set-filter))))

(define (char-set-filter! pred domain base-cs)
  (%set-ranges! base-cs
                (%range-union (%check-char-set base-cs 'char-set-filter!)
                              (%filter-ranges
                               pred
                               (%check-char-set domain 'char-set-filter!)
                               'char-set-filter!)))
  base-cs)

;;; -- Unfold

(define (%unfold-ranges p f g seed proc)
  (%check-procedure p proc)
  (%check-procedure f proc)
  (%check-procedure g proc)
  (%ranges-from-sorted-pairs
   (let loop ((seed seed) (acc '()))
     (if (p seed)
         acc
         (let ((cp (char->integer (%check-char (f seed) proc))))
           (loop (g seed) (cons (cons cp cp) acc)))))))

(define (char-set-unfold p f g seed . maybe-base)
  (%make-char-set
   (%range-union (%default-base maybe-base 'char-set-unfold)
                 (%unfold-ranges p f g seed 'char-set-unfold))))

(define (char-set-unfold! p f g seed base-cs)
  (%set-ranges! base-cs
                (%range-union (%check-char-set base-cs 'char-set-unfold!)
                              (%unfold-ranges p f g seed 'char-set-unfold!)))
  base-cs)

;;; Querying
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

(define (char-set-contains? cs char)
  (%range-contains? (%check-char-set cs 'char-set-contains?)
                    (char->integer (%check-char char 'char-set-contains?))))

(define (char-set-size cs)
  (%range-size (%check-char-set cs 'char-set-size)))

(define (char-set-count pred cs)
  (%check-procedure pred 'char-set-count)
  (%range-fold-ascending (lambda (c n) (if (pred c) (+ n 1) n))
                          0
                          (%check-char-set cs 'char-set-count)))

(define (char-set= . rest)
  (or (null? rest)
      (let ((first (%check-char-set (car rest) 'char-set=)))
        (let loop ((rest (cdr rest)))
          (or (null? rest)
              ;; Normalized range lists are canonical: two sets are equal
              ;; exactly when their lists are.
              (and (equal? first (%check-char-set (car rest) 'char-set=))
                   (loop (cdr rest))))))))

(define (char-set<= . rest)
  (or (null? rest)
      (let loop ((a (%check-char-set (car rest) 'char-set<=))
                 (rest (cdr rest)))
        (or (null? rest)
            (let ((b (%check-char-set (car rest) 'char-set<=)))
              (and (%range-subset? a b)
                   (loop b (cdr rest))))))))

;;; SRFI 14's hash must agree across equal sets and stay below BOUND. The
;;; reference computed it from the 256-character string; here it folds over
;;; the ranges, which are canonical for a given set, so equal sets hash alike
;;; and the cost is per range rather than per member — `char-set:graphic`
;;; hashes in 741 steps instead of 159612.
(define (char-set-hash cs . maybe-bound)
  (let* ((bound (if (pair? maybe-bound) (car maybe-bound) 4194304))
         (bound (if (and (integer? bound) (exact? bound) (> bound 0))
                    bound
                    4194304))
         (ranges (%check-char-set cs 'char-set-hash)))
    (let loop ((rs ranges) (acc 0))
      (if (null? rs)
          (modulo acc bound)
          (loop (cdr rs)
                (modulo (+ (* 37 acc) (caar rs) (* 17 (cdar rs)))
                        ;; Reduce each step so the accumulator cannot grow
                        ;; into a bignum on a large set.
                        4294967296))))))

(define (char-set-every pred cs)
  (%check-procedure pred 'char-set-every)
  (let loop ((rs (%check-char-set cs 'char-set-every)))
    (or (null? rs)
        (let ((lo (caar rs)))
          (let inner ((cp (cdar rs)))
            (cond ((< cp lo) (loop (cdr rs)))
                  ((pred (integer->char cp)) (inner (- cp 1)))
                  (else #f)))))))

(define (char-set-any pred cs)
  (%check-procedure pred 'char-set-any)
  (let loop ((rs (%check-char-set cs 'char-set-any)))
    (and (pair? rs)
         (let ((lo (caar rs)))
           (let inner ((cp (cdar rs)))
             (if (< cp lo)
                 (loop (cdr rs))
                 ;; SRFI 14: the predicate's own value is the result, not #t.
                 (let ((v (pred (integer->char cp))))
                   (or v (inner (- cp 1))))))))))

;;; Cursors
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;;; A cursor is the code point of the member it refers to, and -1 once the set
;;; is exhausted. The walk ascends, matching `char-set-fold` and both
;;; references. A code point is a valid cursor for any set, so nothing about
;;; the set is captured in it — which is what lets `char-set-cursor-next` take
;;; a cursor and a set independently, as the SRFI's signature does.

(define (char-set-cursor cs)
  (%cursor-from (%check-char-set cs 'char-set-cursor) 0))

(define (end-of-char-set? cursor) (< cursor 0))

(define (char-set-ref cs cursor)
  (if (end-of-char-set? cursor)
      (error "Cursor is past the end of the char-set" 'char-set-ref cursor)
      (integer->char cursor)))

(define (char-set-cursor-next cs cursor)
  (if (end-of-char-set? cursor)
      (error "Cursor is past the end of the char-set" 'char-set-cursor-next
             cursor)
      (%cursor-from (%check-char-set cs 'char-set-cursor-next) (+ cursor 1))))

;;; The least member at or above `floor`, or -1 when there is none.
(define (%cursor-from ranges floor)
  (let loop ((rs ranges))
    (if (null? rs)
        -1
        (let ((lo (caar rs)) (hi (cdar rs)))
          (cond ((> lo floor) lo)            ; range wholly above: its bottom
                ((<= floor hi) floor)        ; floor falls inside this range
                (else (loop (cdr rs))))))))  ; range wholly below

;;; Mapping and folding
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

(define (char-set-for-each proc cs)
  (%check-procedure proc 'char-set-for-each)
  (%range-for-each-ascending proc (%check-char-set cs 'char-set-for-each)))

(define (char-set-fold kons knil cs)
  (%check-procedure kons 'char-set-fold)
  (%range-fold-ascending kons knil (%check-char-set cs 'char-set-fold)))

(define (char-set-map proc cs)
  (%check-procedure proc 'char-set-map)
  (%make-char-set
   (%ranges-from-sorted-pairs
    (%range-fold-ascending
     (lambda (c acc)
       (let ((cp (char->integer (%check-char (proc c) 'char-set-map))))
         (cons (cons cp cp) acc)))
     '()
     (%check-char-set cs 'char-set-map)))))

(define (char-set->list cs)
  (%range-fold-ascending cons '() (%check-char-set cs 'char-set->list)))

(define (char-set->string cs)
  (list->string (char-set->list cs)))

;;; Adjoin and delete
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

(define (char-set-adjoin cs . chars)
  (%make-char-set
   (%range-union (%check-char-set cs 'char-set-adjoin)
                 (%chars->ranges chars 'char-set-adjoin))))

(define (char-set-adjoin! cs . chars)
  (%set-ranges! cs
                (%range-union (%check-char-set cs 'char-set-adjoin!)
                              (%chars->ranges chars 'char-set-adjoin!)))
  cs)

(define (char-set-delete cs . chars)
  (%make-char-set
   (%range-difference (%check-char-set cs 'char-set-delete)
                      (%chars->ranges chars 'char-set-delete))))

(define (char-set-delete! cs . chars)
  (%set-ranges! cs
                (%range-difference (%check-char-set cs 'char-set-delete!)
                                   (%chars->ranges chars 'char-set-delete!)))
  cs)

;;; Set algebra
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

(define (%fold-sets op init csets proc)
  (let loop ((acc init) (csets csets))
    (if (null? csets)
        acc
        (loop (op acc (%check-char-set (car csets) proc)) (cdr csets)))))

(define (char-set-complement cs)
  (%make-char-set
   (%range-complement (%check-char-set cs 'char-set-complement))))

(define (char-set-complement! cs)
  (%set-ranges! cs (%range-complement (%check-char-set cs
                                                       'char-set-complement!)))
  cs)

(define (char-set-union . csets)
  (%make-char-set (%fold-sets %range-union '() csets 'char-set-union)))

(define (char-set-union! cs . csets)
  (%set-ranges! cs
                (%fold-sets %range-union
                            (%check-char-set cs 'char-set-union!)
                            csets
                            'char-set-union!))
  cs)

(define (char-set-intersection . csets)
  (if (null? csets)
      (%make-char-set %universe)
      (%make-char-set
       (%fold-sets %range-intersection
                   (%check-char-set (car csets) 'char-set-intersection)
                   (cdr csets)
                   'char-set-intersection))))

(define (char-set-intersection! cs . csets)
  (%set-ranges! cs
                (%fold-sets %range-intersection
                            (%check-char-set cs 'char-set-intersection!)
                            csets
                            'char-set-intersection!))
  cs)

(define (char-set-difference cs . csets)
  (%make-char-set
   (%fold-sets %range-difference
               (%check-char-set cs 'char-set-difference)
               csets
               'char-set-difference)))

(define (char-set-difference! cs . csets)
  (%set-ranges! cs
                (%fold-sets %range-difference
                            (%check-char-set cs 'char-set-difference!)
                            csets
                            'char-set-difference!))
  cs)

;;; Symmetric difference: in exactly one of the two.
(define (%range-xor a b)
  (%range-union (%range-difference a b) (%range-difference b a)))

(define (char-set-xor . csets)
  (%make-char-set (%fold-sets %range-xor '() csets 'char-set-xor)))

(define (char-set-xor! cs . csets)
  (%set-ranges! cs
                (%fold-sets %range-xor
                            (%check-char-set cs 'char-set-xor!)
                            csets
                            'char-set-xor!))
  cs)

(define (char-set-diff+intersection cs . csets)
  (let* ((a (%check-char-set cs 'char-set-diff+intersection))
         (rest (%fold-sets %range-union '() csets
                           'char-set-diff+intersection)))
    (values (%make-char-set (%range-difference a rest))
            (%make-char-set (%range-intersection a rest)))))

(define (char-set-diff+intersection! cs1 cs2 . csets)
  (let* ((a (%check-char-set cs1 'char-set-diff+intersection!))
         (rest (%fold-sets %range-union
                           (%check-char-set cs2 'char-set-diff+intersection!)
                           csets
                           'char-set-diff+intersection!)))
    (%set-ranges! cs1 (%range-difference a rest))
    (%set-ranges! cs2 (%range-intersection a rest))
    (values cs1 cs2)))

;;; The standard character sets
;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;;; Each class comes from `char-set-unicode-ranges`, one Rust scan per class.
;;; The vector it returns is alternating inclusive bounds; `%class` turns that
;;; into a range list. Surrogates are already excluded there, so no clipping
;;; is needed.

(define (%class name)
  (let* ((v (char-set-unicode-ranges name))
         (n (vector-length v)))
    (let loop ((i 0) (acc '()))
      (if (>= i n)
          (%normalize (reverse acc))
          (loop (+ i 2)
                (cons (cons (vector-ref v i) (vector-ref v (+ i 1))) acc))))))

(define char-set:empty (%make-char-set '()))
(define char-set:full (%make-char-set %universe))

(define char-set:lower-case (%make-char-set (%class 'lower-case)))
(define char-set:upper-case (%make-char-set (%class 'upper-case)))
(define char-set:title-case (%make-char-set (%class 'title-case)))
(define char-set:letter (%make-char-set (%class 'alphabetic)))

;;; SRFI 14 defines `char-set:digit` as the Unicode Nd characters, which is
;;; the same set `char-numeric?` and `digit-value` answer for.
(define char-set:digit (%make-char-set (%class 'numeric)))

(define char-set:letter+digit
  (char-set-union char-set:letter char-set:digit))

(define char-set:whitespace (%make-char-set (%class 'whitespace)))
(define char-set:iso-control (%make-char-set (%class 'iso-control)))
(define char-set:punctuation (%make-char-set (%class 'punctuation)))
(define char-set:symbol (%make-char-set (%class 'symbol)))
(define char-set:graphic (%make-char-set (%class 'graphic)))

;;; SRFI 14: printing is graphic plus whitespace.
(define char-set:printing
  (char-set-union char-set:graphic char-set:whitespace))

;;; `char-set:hex-digit` and `char-set:ascii` are fixed by their definitions
;;; rather than by a Unicode class: hex digits are the ASCII ones, and the
;;; ASCII set is the first 128 code points.
(define char-set:hex-digit
  (%make-char-set (%normalize (list (cons 48 57)      ; 0-9
                                    (cons 65 70)      ; A-F
                                    (cons 97 102))))) ; a-f

(define char-set:ascii (%make-char-set (list (cons 0 127))))

;;; SRFI 14's `char-set:blank` is the horizontal whitespace: Zs plus tab.
(define char-set:blank
  (char-set-intersection
   char-set:whitespace
   (char-set-union (%make-char-set (list (cons 9 9)))       ; tab
                   (char-set-difference char-set:whitespace
                                        ;; the vertical whitespace
                                        (%make-char-set
                                         (%normalize
                                          (list (cons 10 13)   ; LF..CR
                                                (cons #x85 #x85)
                                                (cons #x2028 #x2029))))))))
