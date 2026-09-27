(import (scheme base) (prefix (srfi 31) s:))
(import (patina compat smoke))

(check-equal "procedure shorthand supports recursion" 120
  ((s:rec (factorial n)
     (if (= n 0) 1 (* n (factorial (- n 1)))))
   5))
(check-equal "expression form binds its recursive procedure" 6
  ((s:rec sum
     (lambda (items)
       (if (null? items) 0 (+ (car items) (sum (cdr items))))))
   '(1 2 3)))
(check-equal "dotted formals support variadic recursion" 10
  ((s:rec (sum . items)
     (if (null? items) 0 (+ (car items) (apply sum (cdr items)))))
   1 2 3 4))
(check-equal "expression form can return a non-procedure" 42 (s:rec answer 42))
(define self (s:rec object (vector (lambda () object))))
(check-equal "a delayed reference sees the completed recursive value" #t
  (eq? self ((vector-ref self 0))))
(check-equal "recursive name shadows locally and captures outer variables"
  '(13 untouched)
  (let ((loop 'untouched) (offset 10))
    (list ((s:rec (loop n)
             (if (= n 0) offset (+ 1 (loop (- n 1)))))
           3)
          loop)))
(smoke-finish)
