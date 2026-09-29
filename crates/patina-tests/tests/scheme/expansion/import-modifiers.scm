;; #546: import-set grammar accepts the scoped identifiers produced by macros.
;; The caller-supplied and template-introduced forms exercise all four
;; modifiers, their operands, nesting, and imported syntax. Rejection and
;; binding-sharing cases need fresh environments: see spliced_imports.rs.

;; Measured 2026-09-28: both Patina backends and Gauche 0.9.15 pass all
;; 12 rows. Chibi 0.12 passes the six caller-supplied rows, but refuses all
;; six template-introduced imports: its meta-7.scm %resolve-import compares
;; modifier heads with memq and receives syntactic closures here. Omit those
;; forms with cond-expand because test-skip cannot prevent an expansion error.
;; This preserves Chibi's comparison of the caller-supplied forms.

(import (scheme base) (srfi 64))
(test-begin "import-modifiers")

(define-syntax splice (syntax-rules () ((_ form) form)))

(splice (import (only (scheme cxr) caddr)))
(test-equal "caller-supplied only" 3 (caddr '(1 2 3 4)))

(splice (import (except (scheme cxr) caddr)))
(test-equal "caller-supplied except" 4 (cadddr '(1 2 3 4)))

(splice (import (prefix (scheme cxr) caller:)))
(test-equal "caller-supplied prefix" 3 (caller:caddr '(1 2 3 4)))

(splice (import (rename (scheme cxr) (caddr caller-third))))
(test-equal "caller-supplied rename" 3 (caller-third '(1 2 3 4)))

(splice
  (import (rename (prefix (except (only (scheme cxr) caddr cadddr)
                                 cadddr)
                          caller-nested:)
                  (caller-nested:caddr caller-nested-third))))
(test-equal "caller-supplied nested modifiers" 3
  (caller-nested-third '(1 2 3 4)))

(splice (import (rename (only (scheme case-lambda) case-lambda)
                        (case-lambda caller-case-lambda))))
(test-equal "caller-supplied modifier imports syntax" 7
  ((caller-case-lambda ((x) x) ((x y) (+ x y))) 3 4))

;; Template-introduced imports (Chibi cannot expand these; see the header).
(cond-expand
  (chibi)
  (else
   (define-syntax import-only
     (syntax-rules () ((_)
       (import (only (scheme cxr) caaar)))))
   (import-only)
   (test-equal "template-introduced only" 5 (caaar '(((5)))))

   (define-syntax import-except
     (syntax-rules () ((_)
       (import (except (scheme cxr) caaar)))))
   (import-except)
   (test-equal "template-introduced except" '(6) (cdaar '(((5 6)))))

   (define-syntax import-prefix
     (syntax-rules () ((_)
       (import (prefix (scheme cxr) template:)))))
   (import-prefix)
   (test-equal "template-introduced prefix" 3 (template:caddr '(1 2 3 4)))

   (define-syntax import-rename
     (syntax-rules () ((_)
       (import (rename (scheme cxr) (caddr template-third))))))
   (import-rename)
   (test-equal "template-introduced rename" 3 (template-third '(1 2 3 4)))

   (define-syntax import-nested
     (syntax-rules () ((_)
       (import (rename (prefix (except (only (scheme cxr) caddr cadddr)
                                      cadddr)
                               template-nested:)
                       (template-nested:caddr template-nested-third))))))
   (import-nested)
   (test-equal "template-introduced nested modifiers" 3
     (template-nested-third '(1 2 3 4)))

   (define-syntax import-syntax
     (syntax-rules () ((_)
       (import (prefix (only (scheme case-lambda) case-lambda) template:)))))
   (import-syntax)
   (test-equal "template-introduced modifier imports syntax" 7
     ((template:case-lambda ((x) x) ((x y) (+ x y))) 3 4))
  ))

(test-end)
