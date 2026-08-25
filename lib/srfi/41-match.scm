;;; stream-match for SRFI 41.
;;;
;;; PATINA DEVIATION — see this library's `.sld` header. The SRFI's own
;;; reference implementation writes these macros in `syntax-case`, which
;;; Patina does not have yet, so Retropikzel's R7RS port (the rest of this
;;; library, byte-identical) comments `stream-match` out. These are
;;; chibi-scheme's `syntax-rules` equivalents (Alex Shinn, BSD 3-Clause,
;;; `lib/srfi/41.scm`), which need nothing beyond the stream primitives
;;; defined beside them. The one edit: chibi's `(assert (stream? strm))`
;;; becomes the error the SRFI specifies for a non-stream argument.

(define-syntax stream-match
  (syntax-rules ()
    ((stream-match expr clause ...)
     (let ((strm expr))
       (if (not (stream? strm))
           (error "stream-match: non-stream argument" strm))
       (stream-match-next strm clause ...)))))

(define-syntax stream-match-next
  (syntax-rules ()
    ((stream-match-next strm)
     (error "stream-match: pattern failure"))
    ((stream-match-next strm clause . clauses)
     (let ((fail (lambda () (stream-match-next strm . clauses))))
       (stream-match-one strm clause (fail))))))

(define-syntax stream-match-one
  (syntax-rules (_)
    ((stream-match-one strm (() . body) fail)
     (if (stream-null? strm)
         (stream-match-body fail . body)
         fail))
    ((stream-match-one strm (_ . body) fail)
     (stream-match-body fail . body))
    ((stream-match-one strm ((a . b) . body) fail)
     (if (stream-pair? strm)
         (stream-match-one
          (stream-car strm)
          (a
           (stream-match-one (stream-cdr strm) (b . body) fail))
          fail)
         fail))
    ((stream-match-one strm (a . body) fail)
     (let ((a strm))
       (stream-match-body fail . body)))))

(define-syntax stream-match-body
  (syntax-rules ()
    ((stream-match-body fail fender expr)
     (if fender expr fail))
    ((stream-match-body fail expr)
     expr)))
