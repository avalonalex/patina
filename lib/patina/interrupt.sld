;; Patina's interrupt condition (#701): what Ctrl-C, or a host's post through
;; its interpreter's interrupt handle, raises at the next poll. It is raised
;; non-continuably and is not an error object, so `error-object?` answers #f
;; and a guard that catches errors lets it through; `interrupt?` answers #t.
(define-library (patina interrupt)
  (import (only (patina internal errors) interrupt?))
  (export interrupt?))
