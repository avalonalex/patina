;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanu64ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base u64vector->list

(define u64vector->list
  (case-lambda
    ((vec) (u64vector->list* vec 0 (u64vector-length vec)))
    ((vec start) (u64vector->list* vec start (u64vector-length vec)))
    ((vec start end) (u64vector->list* vec start end))))

(define (u64vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (u64vector-ref vec i) list)))))

