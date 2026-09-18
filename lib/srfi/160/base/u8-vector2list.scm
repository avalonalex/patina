;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanu8ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base u8vector->list

(define u8vector->list
  (case-lambda
    ((vec) (u8vector->list* vec 0 (u8vector-length vec)))
    ((vec start) (u8vector->list* vec start (u8vector-length vec)))
    ((vec start end) (u8vector->list* vec start end))))

(define (u8vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (u8vector-ref vec i) list)))))

