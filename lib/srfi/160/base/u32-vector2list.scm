;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanu32ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base u32vector->list

(define u32vector->list
  (case-lambda
    ((vec) (u32vector->list* vec 0 (u32vector-length vec)))
    ((vec start) (u32vector->list* vec start (u32vector-length vec)))
    ((vec start end) (u32vector->list* vec start end))))

(define (u32vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (u32vector-ref vec i) list)))))

