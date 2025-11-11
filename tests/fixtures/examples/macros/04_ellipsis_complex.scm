;;; Complex ellipsis pattern tests
;;; Based on Gauche's macro.scm test suite
;;; These test cases ensure proper handling of patterns like (form var ...) ...

;; Test 1: mixlevel1 - Basic mixed-level ellipsis
;; Pattern variables at different depths
(define-syntax mixlevel1
  (syntax-rules ()
    ((_ (?a ?b ...))
     ((?a ?b) ...))))

(display "Test mixlevel1 with multiple values: ")
(display (mixlevel1 (1 2 3 4 5 6)))
(newline)
;; Expected: ((1 2) (1 3) (1 4) (1 5) (1 6))

(display "Test mixlevel1 with single value: ")
(display (mixlevel1 (1)))
(newline)
;; Expected: ()

;; Test 2: mixlevel2 - Nested mixed-level ellipsis
;; This is the TRUE nested ellipsis: (?b ...) appears under outer ...
(define-syntax mixlevel2
  (syntax-rules ()
    ((_ (?a ?b ...) ...)
     (((?a ?b) ...) ...))))

(display "Test mixlevel2: ")
(display (mixlevel2 (1 2 3 4) (2 3 4 5 6)))
(newline)
;; Expected: (((1 2) (1 3) (1 4)) ((2 3) (2 4) (2 5) (2 6)))

;; Test 3: mixlevel3 - Mixed fixed and variable parts
(define-syntax mixlevel3
  (syntax-rules ()
    ((_ ?a (?b ?c ...) ...)
     (((?a ?b ?c) ...) ...))))

(display "Test mixlevel3: ")
(display (mixlevel3 1 (2 3 4 5 6) (7 8 9 10)))
(newline)
;; Expected: (((1 2 3) (1 2 4) (1 2 5) (1 2 6))
;;            ((1 7 8) (1 7 9) (1 7 10)))

;; Test 4: Simple form with multiple pattern variables
;; This is the pattern needed for letrec: (set! var val) ...
(define-syntax test-form-ellipsis
  (syntax-rules ()
    ((_ ((var val) ...))
     ((set! var val) ...))))

(display "Test form with ellipsis: ")
(display (test-form-ellipsis ((x 1) (y 2) (z 3))))
(newline)
;; Expected: ((set! x 1) (set! y 2) (set! z 3))

;; Test 5: Simple letrec implementation
;; This is the actual pattern from R5RS letrec
(define-syntax simple-letrec
  (syntax-rules ()
    ((_ ((var init) ...) body ...)
     (let ((var #f) ...)
       (set! var init) ...
       body ...))))

(display "Test simple-letrec: ")
(display (simple-letrec ((x 1) (y 2))
           (+ x y)))
(newline)
;; Expected: 3

;; Test 6: Recursive letrec test
(display "Test recursive even/odd with simple-letrec: ")
(display (simple-letrec ((even? (lambda (n)
                                  (if (= n 0)
                                      #t
                                      (odd? (- n 1)))))
                         (odd? (lambda (n)
                                 (if (= n 0)
                                     #f
                                     (even? (- n 1))))))
           (even? 10)))
(newline)
;; Expected: #t
