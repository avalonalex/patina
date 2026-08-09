(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.10.0")
  (license bsd)
  (library
    (name
      (chibi math prime))
    (path "chibi/math/prime.sld")
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (scheme base)
      (scheme inexact)
      (srfi 27)))
  (library
    (name
      (chibi math prime-test))
    (path "chibi/math/prime-test.sld")
    (depends
      (scheme base)
      (chibi math prime)
      (chibi test))
    (use-for test))
  (manual "chibi/math/prime.html")
  (description "Prime and number theoretic utilities.")
  (test "run-tests.scm"))
