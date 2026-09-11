;; What `display` and `write` print for values R7RS gives no external
;; representation: error objects, procedures, parameters, continuations.
;;
;; Migrated from `crates/patina-tests/tests/external_representation.rs` (#193
;; Phase 2), which is deleted. Its two rows about an error *nothing* handles
;; went to `callability.rs`, where unguarded rows live: a program cannot
;; observe its own uncaught error.
;;
;; The standard lets an implementation print these however it likes, so none
;; of the spellings here is required — but *something informative* is, and
;; three of them were `#<unknown>` until issue #181. The writer reached them
;; through a chain of type probes, and a heap variant nobody had added a probe
;; for fell off the end of it: an error object, a continuation, and — on the
;; VM, where a `lambda` compiles to a `VmClosure` rather than the tree-walker's
;; `Procedure` — every user-defined procedure. The same miss had already been
;; fixed twice by adding one more probe, so the writer now dispatches on the
;; heap variant instead, and an unhandled one is a compile error.
;;
;; **So almost every row is Patina's own spelling**, scoped with
;; `(cond-expand (patina) (else (test-skip 1)))`: chibi and Gauche report
;; them as skips. Two rows are portable R7RS and are answered by all four
;; implementations. The spellings still earn a suite file rather than a Rust
;; one: both backends run every row, and the backends allocate different heap
;; objects for the same Scheme value, so a spelling that agrees is a property
;; of the language Patina implements and not of the backend a program happened
;; to run on — which is also what a third backend would be held to.
;;
;; An error object is the one value here with parts, and the rows say where
;; its irritants are rendered as much as how: by the writer that assigns
;; datum labels, so that a circular irritant terminates.

(import (scheme base) (scheme write) (scheme lazy) (scheme case-lambda)
        (scheme eval) (srfi 64))

(test-begin "external-representation")

;; `write` into a string, which is what the Rust harness compared.
(define (written x)
  (let ((port (open-output-string)))
    (write x port)
    (get-output-string port)))

(define (displayed x)
  (let ((port (open-output-string)))
    (display x port)
    (get-output-string port)))

(define (caught thunk) (guard (e (#t e)) (thunk)))

;; ── Error objects (issue #181) ─────────────────────────────────────────────

;; The repro from the issue: `display` said `#<unknown>` on both backends.
;; With no irritants, nothing trails the message.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an error object prints its message and irritants"
  '("#<error-object: boom 1 2>" "#<error-object: boom>")
  (list (written (caught (lambda () (error "boom" 1 2))))
        (written (caught (lambda () (error "boom"))))))

;; The irritants are nested values, so they follow the ambient mode the way a
;; list's elements do: `write` distinguishes a string from a symbol from a
;; character, `display` does not. Only the irritants change — the message is
;; prose about the error, not a datum, and stays unquoted in both.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "the irritants follow the ambient write or display mode"
  '("#<error-object: bad key: foo bar c>"
    "#<error-object: bad key: \"foo\" bar #\\c>")
  (let ((e (caught (lambda () (error "bad key:" "foo" 'bar #\c)))))
    (list (displayed e) (written e))))

;; Printing the message is not a substitute for the accessors, and must not
;; disturb them: the irritants are still there, still separate, still in
;; order. R7RS §6.11 makes these the supported way to read an error object —
;; which is why this row is portable.
(test-equal "the accessors still see message and irritants" '("boom" (1 2) #t)
  (let ((e (caught (lambda () (error "boom" 1 2)))))
    (list (error-object-message e) (error-object-irritants e) (error-object? e))))

;; An error the runtime raised itself, not one `error` built, is the same heap
;; object and prints the same way. The `car` diagnostic's exact wording is
;; not pinned — it is not this file's property, and pinning it would turn any
;; rewording of that message into a failure here.
(test-assert "a runtime-raised error is an error object"
  (error-object? (caught (lambda () (car '())))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "a runtime-raised error prints as one, message included" '(#t #t #t)
  (let* ((s (written (caught (lambda () (car '())))))
         (n (string-length s)))
    (list (and (>= n 16) (string=? (substring s 0 16) "#<error-object: "))
          (char=? (string-ref s (- n 1)) #\>)
          (let loop ((i 0))
            (cond ((> (+ i 4) n) #f)
                  ((string=? (substring s i (+ i 4)) "pair") #t)
                  (else (loop (+ i 1))))))))

;; A circular irritant gets a datum label, like a circular value anywhere
;; else. This is what makes printing the irritants safe at all: the error
;; object is rendered by the writer that already assigns labels. Without that,
;; `(error "cycle" xs)` would be one call away from a non-terminating display.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a circular irritant gets a datum label"
  "#<error-object: cycle #0=(1 2 . #0#)>"
  (let ((xs (list 1 2)))
    (set-cdr! (cdr xs) xs)
    (written (caught (lambda () (error "cycle" xs))))))

;; The error object nests both ways: inside a list, and around a compound
;; irritant. Neither is a special case in the writer — it recurses.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an error object nests in both directions"
  "(#<error-object: nested (1 #(2 3))>)"
  (written (list (caught (lambda () (error "nested" (list 1 (vector 2 3))))))))

;; `write-shared` labels a shared error object, because the writer descends
;; into one. Its contract (R7RS §6.13.3) is to label all shared structure,
;; and a value the writer descends into but refuses to label is re-emitted in
;; full at every occurrence — `e1` four times here, and 2^N times at N levels.
;; Plain `write` labels only what is *circular*, so the same structure is
;; written out twice — the treatment a shared pair gets. `write-simple` uses
;; no labels at all; R7RS lets it diverge on a cycle, so only the ordinary
;; case is asserted for it.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "the three writers treat a shared error object as they treat a pair"
  '("#<error-object: c #1=#<error-object: b #0=#<error-object: a 1> #0#> #1#>"
    "#<error-object: b #<error-object: a 1> #<error-object: a 1>>"
    "#<error-object: boom (1 2) \"x\">")
  (let* ((e1 (caught (lambda () (error "a" 1))))
         (e2 (caught (lambda () (error "b" e1 e1))))
         (e3 (caught (lambda () (error "c" e2 e2))))
         (with (lambda (writer x)
                 (let ((port (open-output-string)))
                   (writer x port)
                   (get-output-string port)))))
    (list (with write-shared e3)
          (with write e2)
          (with write-simple (caught (lambda () (error "boom" (list 1 2) "x")))))))

;; ── Procedures, parameters, continuations ──────────────────────────────────

;; The VM printed `#<unknown>` for every `lambda` a program defined: its
;; closures are a heap variant the probe chain never learned about, while the
;; tree-walker's are the one variant it did. A primitive printed correctly on
;; both, which is why this went unnoticed — and it prints the same as a
;; `lambda`, since R7RS gives a program no way to tell them apart either.
(define (named-procedure x) x)
(cond-expand (patina) (else (test-skip 1)))
(test-equal "every procedure prints as a procedure"
  '("#<procedure>" "#<procedure>" "#<procedure>" "#<procedure>")
  (map written (list (lambda (x) x) named-procedure (case-lambda ((x) x)) car)))

;; A parameter fell through the probe chain to `#<unknown>` until #72 made
;; `procedure?` answer #t for one, at which point a value claiming to be a
;; procedure printed as nothing in particular and the same commit gave it
;; this spelling.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a parameter prints as a parameter" "#<parameter>"
  (written (make-parameter 1)))

;; One spelling on both backends for three different captures: the
;; tree-walker's heap continuation, the VM's handle into its store, and a
;; *delimited* continuation, the one an abort hands its prompt's handler.
;; Anything else would let a program read its backend, or the capture's
;; flavour, out of its output.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "every continuation prints as a continuation"
  '("#<continuation>" "#<continuation>")
  (let ((tag (make-continuation-prompt-tag)))
    (list (written (call-with-current-continuation (lambda (k) k)))
          (written (call-with-continuation-prompt
                     (lambda () (abort-current-continuation tag 1))
                     tag
                     (lambda (v k) k))))))

;; ── The property behind the individual spellings ───────────────────────────

;; No value a program can hold prints as `#<unknown>`, over every value shape
;; this file can reach. The rows above pin one spelling each; this pins the
;; property they exist for, and is the check that would have caught all three
;; misses at once. It answers the *bad* renderings, so the expected value is
;; the empty list and a failure names what went wrong.
;;
;; Each backend answers for itself, which the Rust version had to arrange by
;; hand: one of these renderings legitimately differs between them (a prompt
;; tag prints an allocation-order id), so agreement is not what is asserted.
(define-record-type <point> (make-point x) point? (x point-x))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "nothing reachable prints as #<unknown>" '()
  (let* ((tag (make-continuation-prompt-tag))
         (values-to-print
          (list (caught (lambda () (error "boom" 1 2)))
                (caught (lambda () (car '())))
                (lambda (x) x)
                car
                (case-lambda ((x) x))
                (call-with-current-continuation (lambda (k) k))
                (call-with-continuation-prompt
                  (lambda () (abort-current-continuation tag 1)) tag (lambda (v k) k))
                tag
                (make-parameter 1)
                (delay 1)
                (make-point 1)
                <point>
                (open-input-string "x")
                (current-output-port)
                (environment '(scheme base))
                (string->symbol "sym")
                (bytevector 1 2)
                3.5
                1/2
                (* 1000000000000 1000000000000)))
         (unknown? (lambda (s)
                     (let loop ((i 0))
                       (cond ((> (+ i 10) (string-length s)) #f)
                             ((string=? (substring s i (+ i 10)) "#<unknown>") #t)
                             (else (loop (+ i 1))))))))
    (let loop ((vs values-to-print) (bad '()))
      (if (null? vs)
          (reverse bad)
          (let ((s (written (car vs))))
            (loop (cdr vs)
                  (if (or (= (string-length s) 0) (unknown? s))
                      (cons s bad)
                      bad)))))))

(test-end)
