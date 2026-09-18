;;; SPDX-FileCopyrightText: 2018 John Cowan <cowans32ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base s32vector->list

(define s32vector->list
  (case-lambda
    ((vec) (s32vector->list* vec 0 (s32vector-length vec)))
    ((vec start) (s32vector->list* vec start (s32vector-length vec)))
    ((vec start end) (s32vector->list* vec start end))))

(define (s32vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (s32vector-ref vec i) list)))))

