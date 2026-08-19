;;; SRFI 69: Basic Hash Tables — Reference Implementation
;;;
;;; SPDX-License-Identifier: MIT
;;; Copyright (c) 2005 Panu Kalliokoski. All Rights Reserved.
;;;
;;; Permission is hereby granted, free of charge, to any person obtaining
;;; a copy of this software and associated documentation files (the
;;; "Software"), to deal in the Software without restriction, including
;;; without limitation the rights to use, copy, modify, merge, publish,
;;; distribute, sublicense, and/or sell copies of the Software, and to
;;; permit persons to whom the Software is furnished to do so, subject to
;;; the following conditions:
;;;
;;; The above copyright notice and this permission notice shall be included
;;; in all copies or substantial portions of the Software.
;;;
;;; THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
;;; OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
;;; MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
;;; IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
;;; CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
;;; TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
;;; SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

(define *default-bound* (- (expt 2 29) 3))

(define (%string-hash s ch-conv bound)
  (let ((hash 31)
        (len (string-length s)))
    (do ((index 0 (+ index 1)))
      ((>= index len) (modulo hash bound))
      (set! hash (modulo (+ (* 37 hash)
                            (char->integer (ch-conv (string-ref s index))))
                         *default-bound*)))))

(define (string-hash s . maybe-bound)
  (let ((bound (if (null? maybe-bound) *default-bound* (car maybe-bound))))
    (%string-hash s (lambda (x) x) bound)))

(define (string-ci-hash s . maybe-bound)
  (let ((bound (if (null? maybe-bound) *default-bound* (car maybe-bound))))
    (%string-hash s char-downcase bound)))

(define (symbol-hash s . maybe-bound)
  (let ((bound (if (null? maybe-bound) *default-bound* (car maybe-bound))))
    (%string-hash (symbol->string s) (lambda (x) x) bound)))

(define (hash obj . maybe-bound)
  ;; PATINA DEVIATION: the whole `cond` is wrapped in `exact`.
  ;;
  ;; The result is used as a vector index, by `%hash-table-hash` and again by
  ;; `%hash-table-maybe-resize!`, so an inexact one fails with "vector-ref
  ;; expects an integer index" — any inexact key crashed the table. *Two*
  ;; branches can produce one: `integer?` is true of `2.0`, and `real?` reaches
  ;; `numerator`/`denominator`, which R7RS says return inexact results for
  ;; inexact arguments. Coercing once around the `cond` covers the class,
  ;; including the `vector?`/`pair?`/`number?` branches that recurse back in;
  ;; fixing either branch alone leaves the other, which is how the first
  ;; attempt at this missed `2.0`.
  ;;
  ;; Upstream does not hit it: chibi's SRFI 69 is C-backed and returns exact
  ;; hashes.
  (let ((bound (if (null? maybe-bound) *default-bound* (car maybe-bound))))
    (exact
     (cond ((integer? obj) (modulo (abs obj) bound))
           ((string? obj) (string-hash obj bound))
           ((symbol? obj) (symbol-hash obj bound))
           ((rational? obj) (modulo (abs (+ (numerator obj) (denominator obj)))
                                    bound))
           ;; PATINA DEVIATION: +inf.0, -inf.0 and +nan.0 are `real?` but not
           ;; `rational?`, so upstream's `real?` branch reached `numerator`,
           ;; which raises on them — any such key errored rather than hashing.
           ;; The `exact` wrap above fixed `2.0` and the rationals and left
           ;; these three. Colliding them costs nothing; chibi hashes them too.
           ((real? obj) 0)
           ((number? obj)
            (modulo (+ (hash (real-part obj)) (* 3 (hash (imag-part obj))))
                    bound))
           ((char? obj) (modulo (char->integer obj) bound))
           ((vector? obj) (vector-hash obj bound))
           ((pair? obj) (modulo (+ (hash (car obj)) (* 3 (hash (cdr obj))))
                                bound))
           ((null? obj) 0)
           ((not obj) 0)
           ((procedure? obj) (error "hash: procedures cannot be hashed" obj))
           (else 1)))))

;; PATINA DEVIATION: a real identity hash.
;;
;; Upstream is `(define hash-by-identity hash)`, which hashes *structure* —
;; wrong for the one table that chooses it, `(make-hash-table eq?)`, and
;; newly load-bearing since SRFI 125's `make-eq-comparator` routes here.
;; Three consequences, all of which chibi (whose SRFI 69 is C-backed and
;; hashes by identity) gets right:
;;
;;   (define ht (make-hash-table eq?))
;;   (define k (vector 1 2 3))
;;   (hash-table-set! ht k 'v)
;;   (vector-set! k 0 99)
;;   (hash-table-ref/default ht k 'MISSING)  ;; was MISSING: the key moved
;;
;; a procedure key raised "hash: procedures cannot be hashed", and a circular
;; pair key recursed until the stack ran out.
;;
;; `identity-hash` is `eq?`'s own hash: the heap index, which is stable
;; because the collector does not move objects, and the immediates by value,
;; which is what `eq?` compares them by.
;;
;; `eqv?` tables are deliberately left on the structural `hash`, which is what
;; chibi does too — `(make-hash-table eqv?)` loses a mutated vector key there
;; as well.
(define (hash-by-identity obj . maybe-bound)
  (let ((bound (if (null? maybe-bound) *default-bound* (car maybe-bound))))
    (modulo (identity-hash obj) bound)))

(define (vector-hash v bound)
  (let ((hashvalue 571)
        (len (vector-length v)))
    (do ((index 0 (+ index 1)))
      ((>= index len) (modulo hashvalue bound))
      (set! hashvalue (modulo (+ (* 257 hashvalue) (hash (vector-ref v index)))
                              *default-bound*)))))

(define %make-hash-node cons)
(define %hash-node-set-value! set-cdr!)
(define %hash-node-key car)
(define %hash-node-value cdr)

(define-record-type <srfi-hash-table>
  (%make-hash-table size hash compare associate entries)
  hash-table?
  (size hash-table-size hash-table-set-size!)
  (hash hash-table-hash-function)
  (compare hash-table-equivalence-function)
  (associate hash-table-association-function)
  (entries hash-table-entries hash-table-set-entries!))

(define *default-table-size* 64)

(define (appropriate-hash-function-for comparison)
  (or (and (eq? comparison eq?) hash-by-identity)
      (and (eq? comparison string=?) string-hash)
      (and (eq? comparison string-ci=?) string-ci-hash)
      hash))

(define (make-hash-table . args)
  (let* ((comparison (if (null? args) equal? (car args)))
         (hash
           (if (or (null? args) (null? (cdr args)))
             (appropriate-hash-function-for comparison) (cadr args)))
         (size
           (if (or (null? args) (null? (cdr args)) (null? (cddr args)))
             *default-table-size* (caddr args)))
         (association
           (or (and (eq? comparison eq?) assq)
               (and (eq? comparison eqv?) assv)
               (and (eq? comparison equal?) assoc)
               (letrec
                 ((associate
                    (lambda (val alist)
                      (cond ((null? alist) #f)
                            ((comparison val (caar alist)) (car alist))
                            (else (associate val (cdr alist)))))))
                 associate))))
    (%make-hash-table 0 hash comparison association (make-vector size '()))))

(define (%hash-table-hash hash-table key)
  ((hash-table-hash-function hash-table)
     key (vector-length (hash-table-entries hash-table))))

(define (%hash-table-find entries associate hash key)
  (associate key (vector-ref entries hash)))

(define (%hash-table-add! entries hash key value)
  (vector-set! entries hash
               (cons (%make-hash-node key value)
                     (vector-ref entries hash))))

(define (%hash-table-delete! entries compare hash key)
  (let ((entrylist (vector-ref entries hash)))
    (cond ((null? entrylist) #f)
          ((compare key (caar entrylist))
           (vector-set! entries hash (cdr entrylist)) #t)
          (else
            (let loop ((current (cdr entrylist)) (previous entrylist))
              (cond ((null? current) #f)
                    ((compare key (caar current))
                     (set-cdr! previous (cdr current)) #t)
                    (else (loop (cdr current) current))))))))

(define (%hash-table-walk proc entries)
  (do ((index (- (vector-length entries) 1) (- index 1)))
    ((< index 0)) (for-each proc (vector-ref entries index))))

(define (%hash-table-maybe-resize! hash-table)
  (let* ((old-entries (hash-table-entries hash-table))
         (hash-length (vector-length old-entries)))
    (if (> (hash-table-size hash-table) hash-length)
      (let* ((new-length (* 2 hash-length))
             (new-entries (make-vector new-length '()))
             (hash (hash-table-hash-function hash-table)))
        (%hash-table-walk
          (lambda (node)
            (%hash-table-add! new-entries
                              (hash (%hash-node-key node) new-length)
                              (%hash-node-key node) (%hash-node-value node)))
          old-entries)
        (hash-table-set-entries! hash-table new-entries)))))

(define (hash-table-ref hash-table key . maybe-default)
  (cond ((%hash-table-find (hash-table-entries hash-table)
                           (hash-table-association-function hash-table)
                           (%hash-table-hash hash-table key) key)
         => %hash-node-value)
        ((null? maybe-default)
         (error "hash-table-ref: no value associated with" key))
        (else ((car maybe-default)))))

(define (hash-table-ref/default hash-table key default)
  (hash-table-ref hash-table key (lambda () default)))

(define (hash-table-set! hash-table key value)
  (let ((hash (%hash-table-hash hash-table key))
        (entries (hash-table-entries hash-table)))
    (cond ((%hash-table-find entries
                             (hash-table-association-function hash-table)
                             hash key)
           => (lambda (node) (%hash-node-set-value! node value)))
          (else (%hash-table-add! entries hash key value)
                (hash-table-set-size! hash-table
                                       (+ 1 (hash-table-size hash-table)))
                (%hash-table-maybe-resize! hash-table)))))

(define (hash-table-update! hash-table key function . maybe-default)
  (let ((hash (%hash-table-hash hash-table key))
        (entries (hash-table-entries hash-table)))
    (cond ((%hash-table-find entries
                             (hash-table-association-function hash-table)
                             hash key)
           => (lambda (node)
                (%hash-node-set-value!
                  node (function (%hash-node-value node)))))
          ((null? maybe-default)
           (error "hash-table-update!: no value exists for key" key))
          (else (%hash-table-add! entries hash key
                                  (function ((car maybe-default))))
                (hash-table-set-size! hash-table
                                       (+ 1 (hash-table-size hash-table)))
                (%hash-table-maybe-resize! hash-table)))))

(define (hash-table-update!/default hash-table key function default)
  (hash-table-update! hash-table key function (lambda () default)))

(define (hash-table-delete! hash-table key)
  (if (%hash-table-delete! (hash-table-entries hash-table)
                           (hash-table-equivalence-function hash-table)
                           (%hash-table-hash hash-table key) key)
    (hash-table-set-size! hash-table (- (hash-table-size hash-table) 1))))

(define (hash-table-exists? hash-table key)
  (and (%hash-table-find (hash-table-entries hash-table)
                         (hash-table-association-function hash-table)
                         (%hash-table-hash hash-table key) key) #t))

(define (hash-table-walk hash-table proc)
  (%hash-table-walk
    (lambda (node) (proc (%hash-node-key node) (%hash-node-value node)))
    (hash-table-entries hash-table)))

(define (hash-table-fold hash-table f acc)
  (hash-table-walk hash-table
                       (lambda (key value) (set! acc (f key value acc))))
  acc)

(define (alist->hash-table alist . args)
  (let* ((comparison (if (null? args) equal? (car args)))
         (hash
           (if (or (null? args) (null? (cdr args)))
             (appropriate-hash-function-for comparison) (cadr args)))
         (size
           (if (or (null? args) (null? (cdr args)) (null? (cddr args)))
             (max *default-table-size* (* 2 (length alist))) (caddr args)))
         (hash-table (make-hash-table comparison hash size)))
    (for-each
      (lambda (elem)
        (hash-table-update!/default
          hash-table (car elem) (lambda (x) x) (cdr elem)))
      alist)
    hash-table))

(define (hash-table->alist hash-table)
  (hash-table-fold hash-table
                   (lambda (key val acc) (cons (cons key val) acc)) '()))

(define (hash-table-copy hash-table)
  (let ((new (make-hash-table (hash-table-equivalence-function hash-table)
                              (hash-table-hash-function hash-table)
                              (max *default-table-size*
                                   (* 2 (hash-table-size hash-table))))))
    (hash-table-walk hash-table
                     (lambda (key value) (hash-table-set! new key value)))
    new))

(define (hash-table-merge! hash-table1 hash-table2)
  (hash-table-walk
    hash-table2
    (lambda (key value) (hash-table-set! hash-table1 key value)))
  hash-table1)

(define (hash-table-keys hash-table)
  (hash-table-fold hash-table (lambda (key val acc) (cons key acc)) '()))

(define (hash-table-values hash-table)
  (hash-table-fold hash-table (lambda (key val acc) (cons val acc)) '()))
