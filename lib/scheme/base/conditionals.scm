;; conditionals.scm -- Conditional macros for (scheme base)
;;
;; Control flow macros (R7RS Section 4.2.1)

(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (if (not test) (begin body ...)))))

;; Boolean logic macros (R7RS Section 4.2.1)
;; Short-circuiting and/or operators

(define-syntax and
  (syntax-rules ()
    ((and) #t)
    ((and test) test)
    ((and test1 test2 ...)
     (if test1 (and test2 ...) #f))))

(define-syntax or
  (syntax-rules ()
    ((or) #f)
    ((or test) test)
    ((or test1 test2 ...)
     (let ((or-tmp test1))
       (if or-tmp or-tmp (or test2 ...))))))

;; cond - multi-branch conditional with else and => support
(define-syntax cond
  (syntax-rules (else =>)
    ;; No clauses - unspecified behavior
    ((cond) (if #f #f))

    ;; Single else clause with => (error case)
    ((cond (else => proc))
     (syntax-error "cond: else clause cannot use =>"))

    ;; Single else clause (base case)
    ((cond (else result1 result2 ...))
     (begin result1 result2 ...))

    ;; An else clause with clauses after it. R7RS puts else last; without
    ;; this rule the arms below read `else` as a test expression and the
    ;; mistake surfaced as a misuse of the keyword (#432). Gauche rejects
    ;; it too; chibi says "non-final else in cond".
    ((cond (else . body) clause1 clause ...)
     (syntax-error "cond: an else clause must be the last clause" (else . body)))

    ;; Single test clause with =>
    ((cond (test => proc))
     (let ((temp test))
       (if temp (proc temp))))

    ;; Single test clause without expressions (returns test value)
    ((cond (test))
     test)

    ;; Single test clause with expressions
    ((cond (test result1 result2 ...))
     (if test (begin result1 result2 ...)))

    ;; Multiple clauses with => in first
    ((cond (test => proc) clause ...)
     (let ((temp test))
       (if temp
           (proc temp)
           (cond clause ...))))

    ;; Multiple clauses with bare test in first (returns test value if true)
    ((cond (test) clause ...)
     (let ((temp test))
       (if temp
           temp
           (cond clause ...))))

    ;; Multiple clauses - standard case
    ((cond (test result1 result2 ...) clause ...)
     (if test
         (begin result1 result2 ...)
         (cond clause ...)))

    ;; Anything else is a clause that is not a list. Last, so it only
    ;; names what every rule above refused (#432).
    ((cond clause1 clause ...)
     (syntax-error "cond: a clause must be a list" clause1))))

;; case - pattern matching with eqv? comparison
(define-syntax case
  (syntax-rules (else =>)
    ;; Base case with else and =>
    ((case key (else => proc))
     (proc key))

    ;; An else clause with clauses after it. R7RS puts else last, and
    ;; Gauche rejects this; chibi accepts it and never reaches the clauses
    ;; after. Without this rule it failed as "no pattern matches", with
    ;; nothing to say which clause was wrong (#432).
    ((case key (else . body) clause1 clause ...)
     (syntax-error "case: an else clause must be the last clause" (else . body)))

    ;; Base case with else
    ((case key (else result1 result2 ...))
     (begin result1 result2 ...))

    ;; Empty else — R7RS's grammar wants at least one expression, but both
    ;; chibi and Gauche accept the empty body with an unspecified result,
    ;; and real packages ship it (chibi-tar's `((#\g #\x))` metadata clause
    ;; is the datum-clause sibling below). `key` is evaluated here per R7RS;
    ;; the non-empty else arm above drops it, a pre-existing quirk this arm
    ;; does not inherit.
    ((case key (else))
     (begin key (if #f #f)))

    ;; Base case - no match
    ((case key)
     (if #f #f))

    ;; Single clause with =>
    ((case key ((datum ...) => proc))
     (let ((temp key))
       (if (memv temp '(datum ...))
           (proc temp))))

    ;; Single clause without =>
    ((case key ((datum ...) result1 result2 ...))
     (let ((temp key))
       (if (memv temp '(datum ...))
           (begin result1 result2 ...))))


    ;; Multiple clauses with => in first
    ((case key ((datum ...) => proc) clause ...)
     (let ((temp key))
       (if (memv temp '(datum ...))
           (proc temp)
           (case temp clause ...))))

    ;; Multiple clauses - standard case
    ((case key ((datum ...) result1 result2 ...) clause ...)
     (let ((temp key))
       (if (memv temp '(datum ...))
           (begin result1 result2 ...)
           (case temp clause ...))))

    ;; Empty-body datum clause, alone or followed by more clauses
    ;; (`clause ...` matches zero): delegate to the arms above with an
    ;; explicitly unspecified body, rather than copying their dispatch.
    ((case key ((datum ...)) clause ...)
     (case key ((datum ...) (if #f #f)) clause ...))

    ;; Anything else is a clause that is neither `((datum ...) expr ...)`
    ;; nor an else clause — `(0 'zero)` for `((0) 'zero)` is the usual one.
    ;; Last, so it only names what every rule above refused (#432).
    ((case key clause1 clause ...)
     (syntax-error "case: a clause must be ((datum ...) expression ...) or (else expression ...)"
                   clause1))))
