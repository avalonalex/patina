(package
  (maintainers "Jantony Velazquez Gauthier <jantony.velazquez@upr.edu>")
  (authors "Adam Nelson")
  (version "1.3")
  (library
    (name
      (srfi 197))
    (path "srfi-197.sld")
    (depends
      (scheme base)))
  (manual "srfi-197.html")
  (description "SRFI 197: Pipeline Operators")
  (test "test-r7rs.scm")
  (test-depends
    (scheme base)
    (scheme process-context)
    (scheme write)
    (srfi 2)))
