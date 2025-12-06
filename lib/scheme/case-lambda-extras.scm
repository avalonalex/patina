;;; SRFI-16: case-lambda - syntax for procedures with variable arity
;;; Reference implementation from https://srfi.schemers.org/srfi-16/srfi-16.html
;;;
;;; This implementation uses literal markers ("CLAUSE" and "IMPROPER") to handle
;;; the complex recursive pattern matching needed for case-lambda.

(define-syntax case-lambda
  (syntax-rules ()
    ;; Entry point: no clauses - error
    ((case-lambda)
     (lambda args
       (error "CASE-LAMBDA without any clauses.")))

    ;; Entry point: one or more clauses - wrap in lambda and dispatch
    ((case-lambda
      (?a1 ?e1 ...)
      ?clause1 ...)
     (lambda args
       (let ((l (length args)))
         (case-lambda "CLAUSE" args l
           (?a1 ?e1 ...)
           ?clause1 ...))))

    ;; CLAUSE: Fixed arity - ((?a1 ...) ?e1 ...)
    ;; Check if arg count matches, if so apply, else try next clause
    ((case-lambda "CLAUSE" ?args ?l
      ((?a1 ...) ?e1 ...)
      ?clause1 ...)
     (if (= ?l (length '(?a1 ...)))
         (apply (lambda (?a1 ...) ?e1 ...) ?args)
         (case-lambda "CLAUSE" ?args ?l
           ?clause1 ...)))

    ;; CLAUSE: Variadic - ((?a1 . ?ar) ?e1 ...)
    ;; Transition to IMPROPER mode to count fixed params
    ((case-lambda "CLAUSE" ?args ?l
      ((?a1 . ?ar) ?e1 ...)
      ?clause1 ...)
     (case-lambda "IMPROPER" ?args ?l 1 (?a1 . ?ar) (?ar ?e1 ...)
       ?clause1 ...))

    ;; CLAUSE: Pure variadic - (?a1 ?e1 ...) where ?a1 is just a symbol
    ;; This matches when all args go to a single rest parameter
    ((case-lambda "CLAUSE" ?args ?l
      (?a1 ?e1 ...)
      ?clause1 ...)
     (let ((?a1 ?args))
       ?e1 ...))

    ;; CLAUSE: No more clauses - error
    ((case-lambda "CLAUSE" ?args ?l)
     (error "Wrong number of arguments to CASE-LAMBDA."))

    ;; IMPROPER: Continue counting fixed params in dotted list
    ((case-lambda "IMPROPER" ?args ?l ?k ?al ((?a1 . ?ar) ?e1 ...)
      ?clause1 ...)
     (case-lambda "IMPROPER" ?args ?l (+ ?k 1) ?al (?ar ?e1 ...)
      ?clause1 ...))

    ;; IMPROPER: Done counting - ?ar is the rest param, check and apply
    ((case-lambda "IMPROPER" ?args ?l ?k ?al (?ar ?e1 ...)
      ?clause1 ...)
     (if (>= ?l ?k)
         (apply (lambda ?al ?e1 ...) ?args)
         (case-lambda "CLAUSE" ?args ?l
           ?clause1 ...)))))
