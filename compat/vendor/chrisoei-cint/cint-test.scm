(import 
        (scheme small)
        (srfi 144) ; floating point math functions
        (chibi test)
        (chrisoei cint))

(define (test-approx= a b tolerance)
	(if (number? a)
	; if scalar
		(test-assert "approx=" (< (abs (- a b)) tolerance))
	; else assume a and b are lists
		(begin
			(test "length=" (length a) (length b))
			(map
				(lambda (x y) (test-approx= x y tolerance))
				a
				b
			)
		)
	)
)

(test "sanity check" 1 1)

(test-approx= '(1 2) '(1.000001 1.99999999) 0.001)

(test-approx=
  '(
    1.065187239944521
    -0.06657420249653259
    0.001386962552011096
  )
  (cint 5 3)
  0.00001
)

(define (range n)
  (define (reversed-range i)
    (if (equal? i 0) '(0)
      (cons i (reversed-range (- i 1)))))
  (reverse (reversed-range (- n 1))))

(test "range" (range 3) '(0 1 2))

; Integrate f from x0 to x1 using n equidistant points
(define (integrate f x0 x1 n)
  (define r (range n))
  (define xspan (- x1 x0))
  (define deltax (/ xspan n))
  (define xs (map (lambda (z) (+ x0 (* deltax z))) r))
  (define ys (map f xs))
  (* (/ (apply + ys) n) xspan))

(test-approx= (integrate sin 0.0 fl-pi 1024) 2.0 0.01)

(define (g n)
  (integrate sin 1.0 2.0 n))

(define (fold-list func accum lst)
  (cons accum
    (if (null? lst)
      (list)
      (fold-list func (func accum (car lst)) (cdr lst)))))

(test "fold-list-1"
      (fold-list + 10 '(1 2 3))
      '(10 11 13 16))

(test "fold-list-2" (fold-list (lambda (x y) (* 2 x)) 1 (range 5))
                  '(1 2 4 8 16 32))

(define (nest-list func accum n)
  (cons accum
    (if (= n 0)
      '()
      (nest-list func (func accum) (- n 1))
    )
  )
)

(define (h l)
  (define cx (cint 1 l))
  (define q (nest-list (lambda (x) (* 2 x)) 1 (- l 1)))
  ;(display q)(newline)
  (define v (map g q))
  ;(display v)(newline)
  (define z (map (lambda (x y) (* x y)) (reverse cx) v))
  ;(display z)(newline)
  (apply + z))

; The exact (analytic) answer
(define ans (- (cos 1.0) (cos 2.0)))

; (g 512) and (h 10) calculate the function at the same points,
; but (h 10) is orders of magnitude more accurate
(test-assert "integration" (< (abs (- (h 10) ans))
                              (/ (abs (- (g 512) ans)) 1000000.0)
                           )
)

; vim: set et ff=unix ft=scheme nocp sts=2 sw=2 ts=2:
