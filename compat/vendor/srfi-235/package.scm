(package
  (maintainers "Jantony Velazquez Gauthier <jantony.velazquez@upr.edu>")
  (authors "John Cowan and Arvydas Silanskas")
  (version "1.0")
  (library
    (name
      (srfi 235))
    (path "srfi/235.sld")
    (cond-expand
      (guile
        (depends
          (srfi srfi-1)))
      (else
        (depends
          (srfi 1))))
    (depends
      (scheme base)
      (scheme case-lambda)))
  (manual "srfi-235.html")
  (description "SRFI 235: Combinators")
  (test "srfi-235-test.scm"))
