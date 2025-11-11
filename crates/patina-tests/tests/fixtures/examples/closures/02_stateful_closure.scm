;; Stateful Closures with set!
;; Demonstrates how closures can maintain and modify state

;; Example 1: Simple counter
(define make-counter
  (lambda ()
    (let ((count 0))
      (lambda ()
        (set! count (+ count 1))
        count))))

(define counter (make-counter))
(counter)  ; => 1
(counter)  ; => 2
(counter)  ; => 3

;; Example 2: Bank account with balance
(define make-account
  (lambda (initial-balance)
    (let ((balance initial-balance))
      (lambda (operation amount)
        (if (eq? operation 'deposit)
            (begin
              (set! balance (+ balance amount))
              balance)
            (if (eq? operation 'withdraw)
                (if (>= balance amount)
                    (begin
                      (set! balance (- balance amount))
                      balance)
                    'insufficient-funds)
                'invalid-operation))))))

(define my-account (make-account 100))
(my-account 'deposit 50)   ; => 150
(my-account 'withdraw 25)  ; => 125
(my-account 'withdraw 200) ; => insufficient-funds
(my-account 'deposit 100)  ; => 225

;; Example 3: Multiple independent counters
(define c1 (make-counter))
(define c2 (make-counter))

(c1)  ; => 1
(c1)  ; => 2
(c2)  ; => 1  (independent state!)
(c1)  ; => 3

;; Expected output: 1, 2, 3, 150, 125, insufficient-funds, 225, 1, 2, 1, 3
