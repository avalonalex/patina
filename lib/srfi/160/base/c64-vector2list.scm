;;; SPDX-FileCopyrightText: 2018 John Cowan <cowanc64ccil.org>
;;;
;;; SPDX-License-Identifier: MIT

;;;; Implementation of SRFI 160 base c64vector->list

(define c64vector->list
  (case-lambda
    ((vec) (c64vector->list* vec 0 (c64vector-length vec)))
    ((vec start) (c64vector->list* vec start (c64vector-length vec)))
    ((vec start end) (c64vector->list* vec start end))))

(define (c64vector->list* vec start end)
  (let loop ((i (- end 1))
             (list '()))
    (if (< i start)
      list
      (loop (- i 1) (cons (c64vector-ref vec i) list)))))

