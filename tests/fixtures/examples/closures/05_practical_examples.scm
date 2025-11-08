;; Practical Closure Examples
;; Real-world use cases for closures

;; Example 1: Memoization (caching function results)
(define make-memoized
  (lambda (f)
    (let ((cache '()))
      (lambda (x)
        (let ((cached (assq x cache)))
          (if cached
              (cdr cached)
              (let ((result (f x)))
                (set! cache (cons (cons x result) cache))
                result)))))))

;; Expensive computation example
(define slow-square (lambda (x) (* x x)))
(define fast-square (make-memoized slow-square))

(fast-square 5)  ; => 25 (computed)
(fast-square 5)  ; => 25 (from cache)
(fast-square 10) ; => 100 (computed)

;; Example 2: Timer/Stopwatch
(define make-timer
  (lambda ()
    (let ((start-time 0)
          (running #f))
      (lambda (action)
        (if (eq? action 'start)
            (begin
              (set! running #t)
              'started)
            (if (eq? action 'stop)
                (begin
                  (set! running #f)
                  'stopped)
                (if (eq? action 'is-running?)
                    running
                    'unknown)))))))

(define timer (make-timer))
(timer 'start)       ; => started
(timer 'is-running?) ; => #t
(timer 'stop)        ; => stopped
(timer 'is-running?) ; => #f

;; Example 3: Stack implementation
(define make-stack
  (lambda ()
    (let ((items '()))
      (lambda (operation . args)
        (if (eq? operation 'push)
            (begin
              (set! items (cons (car args) items))
              'pushed)
            (if (eq? operation 'pop)
                (if (null? items)
                    'empty
                    (let ((top (car items)))
                      (set! items (cdr items))
                      top))
                (if (eq? operation 'peek)
                    (if (null? items)
                        'empty
                        (car items))
                    (if (eq? operation 'size)
                        (length items)
                        'unknown))))))))

(define stack (make-stack))
(stack 'push 1)   ; => pushed
(stack 'push 2)   ; => pushed
(stack 'push 3)   ; => pushed
(stack 'size)     ; => 3
(stack 'peek)     ; => 3
(stack 'pop)      ; => 3
(stack 'pop)      ; => 2
(stack 'size)     ; => 1

;; Expected output: 25, 25, 100, started, #t, stopped, #f,
;;                  pushed, pushed, pushed, 3, 3, 3, 2, 1
