;; N-queens in portable R7RS-small Scheme.
;;
;; Places N queens on an N x N board so that no two attack each other,
;; counts every solution, and prints the first one found. For the
;; classic 8x8 board there are 92 solutions.
;;
;; Run:
;;   ./target/release/patina examples/nqueens.scm

(import (scheme base)
        (scheme write))

(define board-size 8)

;; A partial placement is a list of column numbers, most recent first:
;; (car placed) is the queen on the previous row, (cadr placed) the row
;; before that, and so on. A new queen is safe if it shares no column
;; with an earlier queen and no diagonal -- a queen `dist` rows up
;; attacks diagonally when the column difference is exactly `dist`.
(define (safe? col placed)
  (let loop ((placed placed) (dist 1))
    (or (null? placed)
        (let ((earlier (car placed)))
          (and (not (= col earlier))
               (not (= dist (abs (- col earlier))))
               (loop (cdr placed) (+ dist 1)))))))

;; Depth-first search over rows. Returns all solutions, each a list of
;; column numbers from the last row to the first.
(define (queens n)
  (let place ((row 0) (placed '()))
    (if (= row n)
        (list placed)
        (let try ((col 0) (solutions '()))
          (if (= col n)
              solutions
              (try (+ col 1)
                   (if (safe? col placed)
                       (append solutions
                               (place (+ row 1) (cons col placed)))
                       solutions)))))))

(define (display-solution placed n)
  ;; `placed` lists columns from the last row back to the first.
  (for-each
   (lambda (col)
     (do ((c 0 (+ c 1)))
         ((= c n))
       (display (if (= c col) " Q" " .")))
     (newline))
   (reverse placed)))

(define solutions (queens board-size))

(display "First solution found:")
(newline)
(display-solution (car solutions) board-size)
(newline)
(display "Total solutions for ")
(display board-size)
(display " queens: ")
(display (length solutions))
(newline)
