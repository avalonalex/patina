;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanc128ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base c128vector->list

(define c128vector->list
  (case-lambda
    ((vec) (c128vector->list* vec 0 (c128vector-length vec)))
    ((vec start) (c128vector->list* vec start (c128vector-length vec)))
    ((vec start end) (c128vector->list* vec start end))))

(define (c128vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (c128vector-ref vec i) list)))))

