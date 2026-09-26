;; Shallow copies through (scheme base), (srfi 1), and (scheme list) (#426).
;; R7RS §6.4 and SRFI 1 copy the finite spine, retaining its elements and
;; non-pair tail. Rejecting circular cdr chains is Patina's choice for an
;; out-of-domain argument; a cycle through car is still a valid element.
;; Chibi 0.12 exceeded a 128 MiB watchdog without returning from circular
;; base/SRFI 1 copies (2026-09-26); Gauche 0.9.15 signals. Only Chibi skips
;; those calls. The queue rows exercise SRFI 117's use of the same primitive.

(import (scheme base) (srfi 64)
        (prefix (only (srfi 1) list-copy) srfi:)
        (prefix (only (scheme list) list-copy) scheme:)
        (scheme list-queue))

(test-begin "list-copy")

(define (check-copy label copy)
  (test-equal (string-append label ": empty") '() (copy '()))
  (test-equal (string-append label ": atom") 42 (copy 42))
  (test-equal (string-append label ": proper list") '(1 2 3)
    (copy '(1 2 3)))
  (test-equal (string-append label ": dotted list") '(1 2 . 3)
    (copy '(1 2 . 3)))
  (test-assert (string-append label ": every spine pair is fresh")
    (let* ((source (list 1 2 3)) (result (copy source)))
      (and (not (eq? source result))
           (not (eq? (cdr source) (cdr result)))
           (not (eq? (cddr source) (cddr result))))))
  (test-assert (string-append label ": elements are shared")
    (let* ((element (list 'a)) (result (copy (list element))))
      (eq? element (car result))))
  (test-assert (string-append label ": dotted tail is shared")
    (let* ((tail (vector 'tail)) (source (cons 1 (cons 2 tail)))
           (result (copy source)))
      (and (not (eq? source result))
           (not (eq? (cdr source) (cdr result)))
           (eq? tail (cddr result)))))
  (test-equal (string-append label ": mutating copy leaves source intact")
    '(1 2 3)
    (let* ((source (list 1 2 3)) (result (copy source)))
      (set-car! result 'changed)
      (set-cdr! (cdr result) 'changed)
      source))
  (test-assert (string-append label ": cycle through car is an element")
    (let ((source (list #f)))
      (set-car! source source)
      (let ((result (copy source)))
        (and (not (eq? source result))
             (eq? (car result) source)
             (null? (cdr result))))))
  (test-equal (string-append label ": long finite spine") 5000
    (length (copy (make-list 5000 'element))))
  (cond-expand (chibi (test-skip 1)) (else))
  (test-error (string-append label ": rejects a self-cycle") #t
    (let ((source (list 1)))
      (set-cdr! source source)
      (copy source)))
  (cond-expand (chibi (test-skip 1)) (else))
  (test-error (string-append label ": rejects a multi-pair cycle") #t
    (let ((source (list 1 2 3)))
      (set-cdr! (cddr source) source)
      (copy source)))
  (cond-expand (chibi (test-skip 1)) (else))
  (test-error (string-append label ": rejects a cycle after a prefix") #t
    (let ((source (list 1 2 3 4)))
      (set-cdr! (cdr (cddr source)) (cdr source))
      (copy source))))

(check-copy "scheme base" list-copy)
(check-copy "srfi 1" srfi:list-copy)
(check-copy "scheme list" scheme:list-copy)

(test-assert "list-queue-copy of an empty queue"
  (list-queue-empty? (list-queue-copy (list-queue))))
(test-assert "list-queue-copy has a fresh spine"
  (let* ((source (list-queue 1 2)) (result (list-queue-copy source)))
    (and (not (eq? (list-queue-list source) (list-queue-list result)))
         (not (eq? (cdr (list-queue-list source))
                   (cdr (list-queue-list result)))))))
(test-assert "list-queue-copy shares elements"
  (let* ((element (list 'a)) (result (list-queue-copy (list-queue element))))
    (eq? element (list-queue-front result))))
(test-equal "list-queue-copy owns its back pointer" '((1 2) (1 2 3))
  (let* ((source (list-queue 1 2)) (result (list-queue-copy source)))
    (list-queue-add-back! result 3)
    (list (list-queue-list source) (list-queue-list result))))

(test-end)
