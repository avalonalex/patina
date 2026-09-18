;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanf32ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base f32vector->list

(define f32vector->list
  (case-lambda
    ((vec) (f32vector->list* vec 0 (f32vector-length vec)))
    ((vec start) (f32vector->list* vec start (f32vector-length vec)))
    ((vec start end) (f32vector->list* vec start end))))

(define (f32vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (f32vector-ref vec i) list)))))

