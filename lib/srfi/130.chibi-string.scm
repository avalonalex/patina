;; The subset of `(chibi string)` that `(srfi 130)` is written against,
;; inlined here so the shipped tree carries no `(chibi …)` library (#194, #198).
;;
;; Alex Shinn's code, from the sha256-pinned `(chibi string)` 0.9.0 snowball
;; (`string.sld`'s non-chibi `cond-expand` branch and `string.scm`). BSD
;; 3-Clause, the same licence and the same author as `130.scm` beside it; the
;; full text is in `lib/srfi/PROVENANCE.md` § Licences, reproduced there rather
;; than only linked. `string.scm`'s own copyright line is kept below.
;;
;; **This is a subset, not a copy.** The 28 exported names here are exactly
;; what `(srfi 130)` resolved against `(chibi string)`, determined the way #198
;; prescribes -- delete the import and let the loader name what is unbound,
;; rather than matching text, since the two libraries deliberately export many
;; of the same identifiers. Five internal helpers come with them (`->cursor`,
;; `make-char-predicate`, `complement`, `string-index->cursor`,
;; `string-concatenate`). Bodies are verbatim.
;;
;; Verified minimal: dropping any one definition fails upstream's
;; 219-assertion suite. `%string-index->cursor` is the one whose reachability
;; is indirect -- `->cursor`'s other arm never runs here, since cursors are
;; integers so `string-cursor?` is `integer?` -- but `130.sld`'s own
;; `string-index->cursor` wrapper calls it, so it is reached, and it is
;; `%`-prefixed for that reason.
;;
;; **What that check does not establish.** A suite pass says the code paths it
;; runs are bound; it says nothing about free variables on paths it does not
;; reach. `%string-fold`'s multi-string branch is the case in point: it calls
;; SRFI 1's `any`, upstream imports SRFI 1 for it, and nothing here reaches
;; that branch through `(srfi 130)`. `130.sld` therefore imports
;; `(only (srfi 1) any)` deliberately rather than on the loader's say-so.
;;
;; **Seven are `%`-prefixed** — `%string-contains`, `%string-cursor->index`,
;; `%string-cursor-next`, `%string-cursor-prev`, `%string-fold`,
;; `%string-fold-right`, `%string-join`. `130.sld` used to obtain these by
;; `rename`-ing the import, because `(srfi 130)` defines its own procedures of
;; the same names; with the import gone the rename has to happen at the
;; definitions instead. Internal call sites are renamed with them, so this code
;; still calls chibi's definitions and not SRFI 130's near-namesakes.
;;
;; `%string-contains` is upstream's portable fallback, reached because Patina
;; has no `(srfi 13)`. It is the naive O(n*m) scan *and* it allocates a fresh
;; `substring` at every starting position, because R7RS `string=?` takes no
;; index arguments. SRFI 13 ships Knuth-Morris-Pratt (O(n+m)) and its own
;; reference implementation has this naive shape commented out just above it,
;; as the version it replaced. Kept verbatim regardless: every measured caller
;; searches for a short literal needle, no profile says this is hot, and
;; replacing a search algorithm is a change that deserves its own commit and
;; its own measurement rather than riding along with a file move. The cheap fix,
;; if one is ever wanted, is an in-place character loop rather than KMP — same
;; asymptotics, no allocation, a fraction of the risk.

;; strings.scm -- cursor-oriented string library
;; Copyright (c) 2012-2015 Alex Shinn.  All rights reserved.
;; BSD-style license: http://synthcode.com/license.txt

;; ---------------------------------------------------------------------------
;; From `string.sld`: the non-chibi `cond-expand` branch, where cursors are
;; plain integers -- the fast-random-access path the library was written for.
;; ---------------------------------------------------------------------------

(define (%string-cursor->index str i) i)
(define (%string-index->cursor str i) i)
(define string-cursor? integer?)
(define string-cursor<? <)
(define string-cursor>? >)
(define string-cursor=? =)
(define string-cursor<=? <=)
(define string-cursor>=? >=)
(define string-cursor-ref string-ref)
(define (string-cursor-start s) 0)
(define string-cursor-end string-length)
(define (%string-cursor-next s i) (+ i 1))
(define (%string-cursor-prev s i) (- i 1))
(define (substring-cursor s start . o)
  (substring s start (if (pair? o) (car o) (string-length s))))
(define (%string-concatenate orig-ls . o)
  (let ((sep (if (pair? o) (car o) ""))
        (out (open-output-string)))
    (let lp ((ls orig-ls))
      (cond
       ((pair? ls)
        (if (and sep (not (eq? ls orig-ls)))
            (write-string sep out))
        (write-string (car ls) out)
        (lp (cdr ls)))))
    (get-output-string out)))

;; From `string.sld`'s second `cond-expand`: the `else` branch, reached because
;; Patina has no `(srfi 13)` to borrow `string-contains` from. Upstream's
;; comment is upstream's own.
(define (%string-contains a b . o)  ; really, stupidly slow
  (let ((alen (string-length a))
        (blen (string-length b)))
    (let lp ((i (if (pair? o) (car o) 0)))
      (and (<= (+ i blen) alen)
           (if (string=? b (substring a i (+ i blen)))
               i
               (lp (+ i 1)))))))

;; ---------------------------------------------------------------------------
;; From `string.scm`, verbatim apart from the `%` renames described above.
;; ---------------------------------------------------------------------------

;;> \section{High-level API}

;;> The procedures below are similar to those in SRFI 13 or other
;;> string libraries, except instead of receiving and returning
;;> character indexes they use opaque string cursors.

;;> \procedure{(string-null? str)}
;;> Returns true iff \var{str} is equal to the empty string \scheme{""}.

(define (string-null? str)
  (equal? str ""))

(define (->cursor str x)
  (if (string-cursor? x)
      x
      (%string-index->cursor str x)))

(define (make-char-predicate x)
  (cond ((procedure? x) x)
        ((char? x) (lambda (ch) (eq? ch x)))
        ((char-set? x) (lambda (ch) (char-set-contains? x ch)))
        (else (error "invalid character predicate" x))))

(define (complement pred) (lambda (x) (not (pred x))))

;;> Returns true iff \var{check} is true for any character in
;;> \var{str}.  \var{check} can be a procedure, char (to test for
;;> \scheme{char=?} equivalence) or char-set (to test for
;;> \var{char-set-contains?}).  Always returns false if \var{str} is
;;> empty.

;; INHERITED UPSTREAM DEFECT, not a local edit: the `start` argument is used
;; for the emptiness guard below and then *ignored* by the scan, which starts
;; at `string-cursor-start`. So `(string-any char-numeric? "1abc" 1 4)` answers
;; `#t` where SRFI 130 requires `#f`, and `string-every` inherits it by
;; delegation. Present identically in the `(chibi string)` this was taken from,
;; and upstream's 219-assertion suite does not cover the bounded forms.
;; Recorded rather than fixed here: #198 moved this code, and changing what it
;; computes is a conformance fix that wants its own commit and its own test.
(define (string-any check str . o)
  (let ((pred (make-char-predicate check))
        (end (if (and (pair? o) (pair? (cdr o)))
                 (->cursor str (cadr o))
                 (string-cursor-end str))))
    (and (string-cursor>? end (if (pair? o)
                                  (->cursor str (car o))
                                  (string-cursor-start str)))
         (let lp ((i (string-cursor-start str)))
           (let ((i2 (%string-cursor-next str i))
                 (ch (string-cursor-ref str i)))
             (if (string-cursor>=? i2 end)
                 (pred ch)  ;; tail call
                 (or (pred ch) (lp i2))))))))

;;> Returns true iff \var{check} is true for every character in
;;> \var{str}.  \var{check} can be a procedure, char or char-set as in
;;> \scheme{string-any}.  Always returns true if \var{str} is empty.

(define (string-every check str . o)
  (not (apply string-any (complement (make-char-predicate check)) str o)))

;;> Returns a cursor pointing to the first position from the left in
;;> string for which \var{check} is true.  \var{check} can be a
;;> procedure, char or char-set as in \scheme{string-any}.  The
;;> optional cursors \var{start} and \var{end} can specify a substring
;;> to search, and default to the whole string.  Returns a cursor just
;;> past the end of \var{str} if no character matches.

(define (string-find str check . o)
  (let ((pred (make-char-predicate check))
        (end (if (and (pair? o) (pair? (cdr o)))
                 (->cursor str (cadr o))
                 (string-cursor-end str))))
    (let lp ((i (if (pair? o) (car o) (string-cursor-start str))))
      (cond ((string-cursor>=? i end) end)
            ((pred (string-cursor-ref str i)) i)
            (else (lp (%string-cursor-next str i)))))))

;;> As \scheme{string-find}, but returns the position of the first
;;> character from the right of \var{str}.  If no character matches,
;;> returns a string cursor pointing just before \var{start}.

(define (string-find-right str check . o)
  (let ((pred (make-char-predicate check))
        (start (if (pair? o) (->cursor str (car o)) (string-cursor-start str))))
    (let lp ((i (if (and (pair? o) (pair? (cdr o)))
                    (->cursor str (cadr o))
                    (string-cursor-end str))))
      (let ((i2 (%string-cursor-prev str i)))
        (cond ((string-cursor<? i2 start) start)
              ((pred (string-cursor-ref str i2)) i)
              (else (lp i2)))))))

;;> As \scheme{string-find}, but inverts the check, returning the
;;> position of the first character which doesn't match.

(define (string-skip str check . o)
  (apply string-find str (complement (make-char-predicate check)) o))

;;> As \scheme{string-find-right}, but inverts the check, returning
;;> the position of the first character which doesn't match.

(define (string-skip-right str check . o)
  (apply string-find-right str (complement (make-char-predicate check)) o))

;;> \procedure{(string-join list-of-strings [separator])}
;;>
;;> Concatenates the \var{list-of-strings} and return the result as a
;;> single string.  If \var{separator} is provided it is inserted
;;> between each pair of strings.

(define %string-join %string-concatenate)

;;> Returns two values: the first cursors from the left in
;;> \var{prefix} and in \var{str} where the two strings don't match.

(define (string-mismatch prefix str)
  (let ((end1 (string-cursor-end prefix))
        (end2 (string-cursor-end str)))
    (let lp ((i (string-cursor-start prefix))
             (j (string-cursor-start str)))
      (if (or (string-cursor>=? i end1)
              (string-cursor>=? j end2)
              (not (eq? (string-cursor-ref prefix i) (string-cursor-ref str j))))
          (values i j)
          (lp (%string-cursor-next prefix i) (%string-cursor-next str j))))))

;;> Returns two values: the first cursors from the right in
;;> \var{prefix} and in \var{str} where the two strings don't match.

(define (string-mismatch-right suffix str)
  (let ((end1 (string-cursor-start suffix))
        (end2 (string-cursor-start str)))
    (let lp ((i (%string-cursor-prev suffix (string-cursor-end suffix)))
             (j (%string-cursor-prev str (string-cursor-end str))))
      (if (or (string-cursor<? i end1)
              (string-cursor<? j end2)
              (not (eq? (string-cursor-ref suffix i) (string-cursor-ref str j))))
          (values i j)
          (lp (%string-cursor-prev suffix i) (%string-cursor-prev str j))))))

;;> The fundamental string iterator.  Calls \var{kons} on each
;;> character of \var{str} and an accumulator, starting with
;;> \var{knil}.  If multiple strings are provided, calls \var{kons} on
;;> the corresponding characters of all strings, with the accumulator
;;> as the final argument, and terminates when the shortest string
;;> runs out.

(define (%string-fold kons knil str . los)
  (if (null? los)
      (let ((end (string-cursor-end str)))
        (let lp ((i (string-cursor-start str)) (acc knil))
          (if (string-cursor>=? i end)
              acc
              (lp (%string-cursor-next str i)
                  (kons (string-cursor-ref str i) acc)))))
      (let ((los (cons str los)))
        (let lp ((is (map string-cursor-start los))
                 (acc knil))
          (if (any (lambda (str i)
                     (string-cursor>=? i (string-cursor-end str)))
                   los is)
              acc
              (lp (map %string-cursor-next los is)
                  (apply kons (append (map string-cursor-ref los is)
                                      (list acc)))))))))

;;> Equivalent to \scheme{string-fold}, but iterates over \var{str}
;;> from right to left.

(define (%string-fold-right kons knil str)
  (let ((end (string-cursor-end str)))
    (let lp ((i (string-cursor-start str)))
      (if (string-cursor>=? i end)
          knil
          (kons (string-cursor-ref str i) (lp (%string-cursor-next str i)))))))

;;> \section{Cursor API}

;;> \procedure{(substring-cursor str i [j])}
;;>
;;> Returns the substring of \var{str} between \var{i} (inclusive) and
;;> optional \var{j} (exclusive), which defaults to the end of the
;;> string.

;;> \procedure{(string-cursor-ref str i)}
;;>
;;> Returns the character of \var{str} at position \var{i}.

;;> \procedure{(string-cursor-start str)}
;;>
;;> Returns a string cursor pointing to the start of \var{str}.

;;> \procedure{(string-cursor-end str)}
;;>
;;> Returns a string cursor pointing just past the end of \var{str}.

;;> \procedure{(string-cursor-next str i)}
;;>
;;> Returns a string cursor to the character in \var{str} just after
;;> the cursor \var{i}.

;;> \procedure{(string-cursor-prev str i)}
;;>
;;> Returns a string cursor to the character in \var{str} just before
;;> the cursor \var{i}.

(define (string-cursor-forward str cursor n)
  (if (positive? n)
      (string-cursor-forward str (%string-cursor-next str cursor) (- n 1))
      cursor))

(define (string-cursor-back str cursor n)
  (if (positive? n)
      (string-cursor-back str (%string-cursor-prev str cursor) (- n 1))
      cursor))
