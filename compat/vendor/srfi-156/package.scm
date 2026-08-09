(package
  (maintainers "Robert Fisher")
  (authors "Panicz Maciej Godek")
  (version "1.0.0")
  (library
    (name
      (srfi 156))
    (path "srfi/156.sld")
    (depends
      (scheme base)))
  (library
    (name
      (srfi 156 test))
    (path "srfi/156/test.sld")
    (depends
      (scheme base)
      (srfi 156)
      (chibi test))
    (use-for test))
  (manual "srfi/156.html")
  (description "Reference implementation of SRFI-156: Syntactic combiners for binary predicates")
  (test "run-tests.scm"))
