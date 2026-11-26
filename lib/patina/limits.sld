;; Patina's limit conditions (#907). `&stack-exhausted` is what a call
;; raises when it takes the VM's stack to within 1 MiB of its cap, which
;; `--stack-max`, `PATINA_STACK_MAX` or a host's configuration sets, and
;; which otherwise is fitted to the machine's memory. It is raised
;; non-continuably and it is an error object: `error-object?` answers #t, its
;; message is "stack exhausted" and its one irritant is the cap in bytes.
;; `stack-exhausted?` tells it from other errors. The last MiB under the cap
;; is the room its handler runs in; a handler that runs through that too
;; loses the form. The tree-walker keeps no cap and never raises it.
(define-library (patina limits)
  (import (only (patina internal errors) stack-exhausted?))
  (export stack-exhausted?))
