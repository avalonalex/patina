;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanu16ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base u16vector->list

(define u16vector->list
  (case-lambda
    ((vec) (u16vector->list* vec 0 (u16vector-length vec)))
    ((vec start) (u16vector->list* vec start (u16vector-length vec)))
    ((vec start end) (u16vector->list* vec start end))))

(define (u16vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (u16vector-ref vec i) list)))))

