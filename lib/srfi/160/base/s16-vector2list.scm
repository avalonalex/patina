;;; SPDX-FileCopyrightText: 2018 John Cowan <cowans16ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base s16vector->list

(define s16vector->list
  (case-lambda
    ((vec) (s16vector->list* vec 0 (s16vector-length vec)))
    ((vec start) (s16vector->list* vec start (s16vector-length vec)))
    ((vec start end) (s16vector->list* vec start end))))

(define (s16vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (s16vector-ref vec i) list)))))

