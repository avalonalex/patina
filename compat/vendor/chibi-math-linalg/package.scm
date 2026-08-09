(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.3")
  (license bsd)
  (library
    (name
      (chibi math linalg))
    (path "chibi/math/linalg.sld")
    (cond-expand
      ((and chibi (not no-ffi))
        (depends
          (chibi)
          (srfi 160 base)
          (srfi 231 base)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme inexact)
      (scheme list)
      (scheme write)
      (srfi 33)
      (srfi 231)
      (chibi assert)
      (chibi optional)))
  (library
    (name
      (chibi math linalg-test))
    (path "chibi/math/linalg-test.sld")
    (depends
      (scheme base)
      (scheme list)
      (srfi 231)
      (chibi math linalg)
      (chibi test))
    (use-for test))
  (test "run-tests.scm"))
