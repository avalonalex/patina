;;; SPDX-FileCopyrightText: 2018 John Cowan <cowans64ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base s64vector->list

(define s64vector->list
  (case-lambda
    ((vec) (s64vector->list* vec 0 (s64vector-length vec)))
    ((vec start) (s64vector->list* vec start (s64vector-length vec)))
    ((vec start end) (s64vector->list* vec start end))))

(define (s64vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (s64vector-ref vec i) list)))))

