;; Test hygiene with let-syntax
;; The macro m should capture 'outer because x is in scope when m is defined
;; Even though x is shadowed by 'inner in the inner let, the macro should see 'outer

(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))
