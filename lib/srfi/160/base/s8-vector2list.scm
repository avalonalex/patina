;;; SPDX-FileCopyrightText: 2018 John Cowan <cowans8ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base s8vector->list

(define s8vector->list
  (case-lambda
    ((vec) (s8vector->list* vec 0 (s8vector-length vec)))
    ((vec start) (s8vector->list* vec start (s8vector-length vec)))
    ((vec start end) (s8vector->list* vec start end))))

(define (s8vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (s8vector-ref vec i) list)))))

