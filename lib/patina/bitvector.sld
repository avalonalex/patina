;; Bit vectors, one bit per element, for `(srfi 231)`'s `u1-storage-class`.
;;
;; Patina-authored. SRFI 231 exports a `u1-storage-class` and builds it from
;; six `u1vector-*` procedures, which it expects the host to supply. **SRFI 160
;; defines none of them** — its twelve homogeneous vector types begin at u8 —
;; so an implementation bundling SRFI 231 has to provide them itself. chibi
;; does, as an extension: three of the six (`u1vector?`, `u1vector-ref`,
;; `u1vector-set!`) come from C, re-exported by its `lib/srfi/160/prims.sld`
;; from `uvprims.stub`, and the other three are ordinary Scheme in its
;; `lib/srfi/160/base.sld`. The bit-packed `ref`/`set!`/`?` are therefore the
;; part that actually had to be written here.
;;
;; Why this namespace, and not `(srfi 160 u1)` — which is what chibi calls it.
;; Publishing a `u1` under SRFI 160's namespace would put a name the SRFI does
;; not define into a SRFI's public library space, which is the sort of thing
;; the bundling policy in `PRD/phase2/R7RS_LARGE_STATUS.md` keeps out. Under
;; `(patina …)` the type is plainly ours, and nothing outside `(srfi 231)`
;; imports it. If a standard-track `u1` ever appears, this becomes its
;; implementation rather than a name to retract.
;;
;; The elements really are bits: eight to a byte in a bytevector, which is
;; what the type is *for*. A byte-per-element version would satisfy every
;; SRFI 231 operation — the storage class only needs `ref`, `set!` and
;; `length` to agree — and would use eight times the memory the name promises,
;; so anyone reaching for a large bit mask would get the opposite of what they
;; asked for.

(define-library (patina bitvector)
  (import (scheme base)
          ;; `bitwise-and`, `bitwise-ior`, `bitwise-not` and
          ;; `arithmetic-shift`; `(scheme base)` has none of them.
          (only (srfi 151) bitwise-and bitwise-ior bitwise-not
                arithmetic-shift))
  (export make-u1vector u1vector u1vector? u1? u1vector-length
          u1vector-ref u1vector-set! u1vector->list list->u1vector)
  (include "bitvector.scm"))
