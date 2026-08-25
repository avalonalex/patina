;;; stream-match for SRFI 41.
;;;
;;; PATINA DEVIATION — see this library's `.sld` header. The SRFI's own
;;; reference implementation writes these macros in `syntax-case`, which
;;; Patina does not have yet, so Retropikzel's R7RS port (the rest of this
;;; library, byte-identical) comments `stream-match` out. These are
;;; chibi-scheme's `syntax-rules` equivalents, from *chibi's* own
;;; `lib/srfi/41.scm` — a different file from the one of that name here
;;; (Alex Shinn, BSD 3-Clause) — which need nothing beyond the stream
;;; primitives defined beside them.
;;;
;;; PATINA LOCAL EDIT: chibi's `(assert (stream? strm))` becomes the error
;;; the SRFI specifies for a non-stream argument.
;;;
;;; The nesting is chibi's shape, kept as found: an n-element pattern
;;; expands to O(n^2) code because each level re-substitutes the whole
;;; `(stream-cdr …)` chain instead of binding it. Promises memoize, so the
;;; cost is compile-time size rather than repeated traversal, and matching
;;; a five-element pattern is what the suites do.

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
