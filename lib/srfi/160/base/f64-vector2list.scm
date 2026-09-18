;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanf64ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base f64vector->list

(define f64vector->list
  (case-lambda
    ((vec) (f64vector->list* vec 0 (f64vector-length vec)))
    ((vec start) (f64vector->list* vec start (f64vector-length vec)))
    ((vec start end) (f64vector->list* vec start end))))

(define (f64vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (f64vector-ref vec i) list)))))

