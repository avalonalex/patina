;; Sudoku solver in portable R7RS-small Scheme.
;;
;; Reads puzzles in the Project Euler problem 96 text format
;; (https://projecteuler.net/problem=96): each puzzle is a "Grid NN"
;; header line followed by nine rows of nine digits, with 0 marking an
;; empty cell. Solves every grid in examples/sudoku.txt and prints the
;; sum of the three-digit numbers in the top-left corner of each
;; solution -- the quantity Project Euler asks for.
;;
;; The solver is a depth-first search with two classic refinements:
;; occupancy tables give O(1) legality checks, and the most-constrained
;; cell is always filled first, so forced moves propagate immediately.
;;
;; The bundled puzzle file contains four grids: Grid 01 is the sample
;; grid from the Project Euler problem statement, Grid 02 is the classic
;; easy puzzle from the Wikipedia "Sudoku" article, and Grids 03 and 04
;; are Arto Inkala's "AI Escargot" (2006) and his 2012 puzzle, both
;; famously difficult. Expected output sum: 1991.
;;
;; Run from the repository root:
;;   ./target/release/patina examples/sudoku.scm
;; or with the tree-walking backend:
;;   ./target/release/patina --tree-walker examples/sudoku.scm
;;
;; The two Inkala grids dominate the runtime: the VM backend solves all
;; four in about a second, the tree-walker takes half a minute.

(import (scheme base)
        (scheme file)
        (scheme write))

;; ---------------------------------------------------------------------
;; Puzzle state. The board is a vector of 81 exact integers in row-major
;; order, 0 for an empty cell. Alongside it, three occupancy tables
;; record which digits are already used in each row, column, and 3x3
;; box, so testing whether a digit may be placed costs three lookups
;; instead of a scan.

(define-record-type <sudoku>
  (make-sudoku board rows cols boxes)
  sudoku?
  (board sudoku-board)
  (rows sudoku-rows)
  (cols sudoku-cols)
  (boxes sudoku-boxes))

;; Each table holds nine units of ten booleans (digit 0 unused).
(define (make-table) (make-vector 90 #f))

(define (table-ref table unit digit)
  (vector-ref table (+ (* unit 10) digit)))

(define (table-set! table unit digit used?)
  (vector-set! table (+ (* unit 10) digit) used?))

(define (box-of row col)
  (+ (* 3 (quotient row 3)) (quotient col 3)))

(define (board-ref s row col)
  (vector-ref (sudoku-board s) (+ (* row 9) col)))

(define (mark! s row col digit used?)
  (table-set! (sudoku-rows s) row digit used?)
  (table-set! (sudoku-cols s) col digit used?)
  (table-set! (sudoku-boxes s) (box-of row col) digit used?))

(define (place! s row col digit)
  (vector-set! (sudoku-board s) (+ (* row 9) col) digit)
  (mark! s row col digit #t))

(define (unplace! s row col digit)
  (vector-set! (sudoku-board s) (+ (* row 9) col) 0)
  (mark! s row col digit #f))

(define (board->sudoku board)
  (let ((s (make-sudoku board (make-table) (make-table) (make-table))))
    (do ((idx 0 (+ idx 1)))
        ((= idx 81) s)
      (let ((digit (vector-ref board idx)))
        (unless (zero? digit)
          (mark! s (quotient idx 9) (remainder idx 9) digit #t))))))

;; ---------------------------------------------------------------------
;; Search.

(define (digit-ok? s row col digit)
  (not (or (table-ref (sudoku-rows s) row digit)
           (table-ref (sudoku-cols s) col digit)
           (table-ref (sudoku-boxes s) (box-of row col) digit))))

(define (candidates s row col)
  (let loop ((digit 9) (acc '()))
    (if (zero? digit)
        acc
        (loop (- digit 1)
              (if (digit-ok? s row col digit)
                  (cons digit acc)
                  acc)))))

(define (count-candidates s row col)
  (let loop ((digit 9) (count 0))
    (if (zero? digit)
        count
        (loop (- digit 1)
              (if (digit-ok? s row col digit)
                  (+ count 1)
                  count)))))

;; Find the empty cell with the fewest legal digits (the classic
;; most-constrained-cell heuristic). Returns (row . col), or #f when the
;; board is full. A cell with zero or one candidate is chosen
;; immediately: zero means this branch is dead, one is a forced move.
(define (best-empty-cell s)
  (let loop ((idx 0) (best #f) (best-count 10))
    (if (= idx 81)
        best
        (if (zero? (vector-ref (sudoku-board s) idx))
            (let* ((row (quotient idx 9))
                   (col (remainder idx 9))
                   (count (count-candidates s row col)))
              (cond ((<= count 1) (cons row col))
                    ((< count best-count)
                     (loop (+ idx 1) (cons row col) count))
                    (else (loop (+ idx 1) best best-count))))
            (loop (+ idx 1) best best-count)))))

;; Depth-first search. Mutates the puzzle; returns #t leaving it solved,
;; or #f leaving it exactly as it was.
(define (solve! s)
  (let ((cell (best-empty-cell s)))
    (if (not cell)
        #t
        (let ((row (car cell))
              (col (cdr cell)))
          (let try ((digits (candidates s row col)))
            (cond ((null? digits) #f)
                  (else
                   (place! s row col (car digits))
                   (if (solve! s)
                       #t
                       (begin
                         (unplace! s row col (car digits))
                         (try (cdr digits)))))))))))

;; ---------------------------------------------------------------------
;; Reading the Project Euler puzzle format.

;; A puzzle row is a line starting with nine digit characters.
(define (grid-row? line)
  (and (>= (string-length line) 9)
       (let loop ((i 0))
         (or (= i 9)
             (let ((c (string-ref line i)))
               (and (char<=? #\0 c #\9)
                    (loop (+ i 1))))))))

(define (parse-row! board row line)
  (do ((col 0 (+ col 1)))
      ((= col 9))
    (vector-set! board (+ (* row 9) col)
                 (- (char->integer (string-ref line col))
                    (char->integer #\0)))))

;; Read every grid from `port`, returning a list of (name . board)
;; pairs. Any non-row line (e.g. "Grid 01") names the grid after it.
(define (read-grids port)
  (let loop ((name "Grid") (board #f) (row 0) (grids '()))
    (let ((line (read-line port)))
      (cond ((eof-object? line)
             (reverse grids))
            ((grid-row? line)
             (let ((board (or board (make-vector 81 0))))
               (parse-row! board row line)
               (if (= row 8)
                   (loop name #f 0 (cons (cons name board) grids))
                   (loop name board (+ row 1) grids))))
            ((string=? line "")
             (loop name board row grids))
            (else
             (loop line board row grids))))))

;; ---------------------------------------------------------------------
;; Output.

(define (display-board s)
  (do ((row 0 (+ row 1)))
      ((= row 9))
    (when (and (positive? row) (zero? (remainder row 3)))
      (display "------+-------+------")
      (newline))
    (do ((col 0 (+ col 1)))
        ((= col 9))
      (when (and (positive? col) (zero? (remainder col 3)))
        (display "| "))
      (display (board-ref s row col))
      (display " "))
    (newline)))

(define (top-left-number s)
  (+ (* 100 (board-ref s 0 0))
     (* 10 (board-ref s 0 1))
     (board-ref s 0 2)))

;; ---------------------------------------------------------------------
;; Main program.

(define (find-puzzle-file)
  (cond ((file-exists? "examples/sudoku.txt") "examples/sudoku.txt")
        ((file-exists? "sudoku.txt") "sudoku.txt")
        (else (error "cannot find sudoku.txt; run from the repository root"))))

(define grids (call-with-input-file (find-puzzle-file) read-grids))

(define total
  (let loop ((grids grids) (sum 0))
    (if (null? grids)
        sum
        (let ((name (caar grids))
              (s (board->sudoku (cdar grids))))
          (display name)
          (newline)
          (cond ((solve! s)
                 (display-board s)
                 (newline)
                 (loop (cdr grids) (+ sum (top-left-number s))))
                (else
                 (display "  no solution!")
                 (newline)
                 (loop (cdr grids) sum)))))))

(display "Sum of top-left corner numbers: ")
(display total)
(newline)
