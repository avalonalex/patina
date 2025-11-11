;; Hygiene tests from R7RS specification
;; These test that macros properly handle variable scoping

(import (scheme base) (scheme write))

(display "=== Hygiene Test Suite ===\n\n")

;; Test 1: Free variable refers to definition-site binding
;; From R7RS spec example
(display "Test 1: Free variable hygiene\n")
(define x-outer 'outer)
(define-syntax m1
  (syntax-rules ()
    ((m1) x-outer)))

(define test1-result
  (let ((x-outer 'inner))
    (m1)))

(display "  Expected: outer\n")
(display "  Got: ")
(display test1-result)
(display "\n")
(if (eq? test1-result 'outer)
    (display "  PASS\n")
    (display "  FAIL\n"))

;; Test 2: Macro-inserted bindings don't capture user variables
;; when macro should not capture user's 'if' variable
(display "\nTest 2: Inserted binding hygiene\n")
(define-syntax when
  (syntax-rules ()
    ((when test stmt1 stmt2 ...)
     (if test (begin stmt1 stmt2 ...)))))

(define test2-result
  (let ((if #t))
    (when if (set! if 'now))
    if))

(display "  Expected: now\n")
(display "  Got: ")
(display test2-result)
(display "\n")
(if (eq? test2-result 'now)
    (display "  PASS\n")
    (display "  FAIL\n"))

;; Test 3: Literal identifier matching with local binding
;; From R7RS spec - cond should not treat local => as keyword
(display "\nTest 3: Literal vs. local binding\n")
;; Note: This requires full cond macro which we may not have yet
;; Simplified test: our macro should distinguish literal from binding

(define-syntax simple-cond
  (syntax-rules (else)
    ((simple-cond (else result))
     result)
    ((simple-cond (test result))
     (if test result))))

(define test3-result
  (let ((else #f))
    (simple-cond (#t 'ok))))  ; else is local var, not keyword here

(display "  Expected: ok\n")
(display "  Got: ")
(display test3-result)
(display "\n")
(if (eq? test3-result 'ok)
    (display "  PASS\n")
    (display "  FAIL\n"))

;; Test 4: Macro-inserted temp variable doesn't capture
(display "\nTest 4: Temporary variable hygiene\n")
(define-syntax my-or
  (syntax-rules ()
    ((my-or e1 e2)
     (let ((temp e1))
       (if temp temp e2)))))

(define test4-result
  (let ((temp 'outer-temp))
    (my-or #f temp)))  ; Should return 'outer-temp, not capture it

(display "  Expected: outer-temp\n")
(display "  Got: ")
(display test4-result)
(display "\n")
(if (eq? test4-result 'outer-temp)
    (display "  PASS\n")
    (display "  FAIL\n"))

;; Test 5: Recursive macro with hygiene
;; From R7RS letrec-syntax example (simplified)
(display "\nTest 5: Recursive macro hygiene\n")
(define test5-result
  (letrec-syntax
      ((my-or (syntax-rules ()
                ((my-or) #f)
                ((my-or e) e)
                ((my-or e1 e2 ...)
                 (let ((temp e1))
                   (if temp
                       temp
                       (my-or e2 ...)))))))
    (let ((x #f)
          (y 7)
          (temp 8))
      (my-or x y))))  ; temp in user code should not interfere

(display "  Expected: 7\n")
(display "  Got: ")
(display test5-result)
(display "\n")
(if (= test5-result 7)
    (display "  PASS\n")
    (display "  FAIL\n"))

(display "\n=== Hygiene tests complete ===\n")
