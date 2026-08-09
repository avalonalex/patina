(package
  (maintainers "Kevin Wortman <kwortman@gmail.com>")
  (authors "John Cowan")
  (version "1.0.0")
  (library
    (name
      (comparators))
    (path "comparators.sld")
    (depends
      (scheme case-lambda)
      (scheme base)))
  (manual "srfi-128/srfi-128.html")
  (description "SRFI 128: Comparators (reduced) reference implementation")
  (test "srfi-128/comparators/comparators-test.scm"))
