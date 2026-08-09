(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi bytevector))
    (path "chibi/bytevector.sld")
    (cond-expand
      (big-endian
        (depends))
      (else
        (depends)))
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
    (cond-expand
      (chibi
        (depends
          (scheme bytevector)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme inexact)))
  (library
    (name
      (chibi bytevector-test))
    (path "chibi/bytevector-test.sld")
    (depends
      (scheme base)
      (chibi bytevector)
      (chibi test))
    (use-for test))
  (manual "chibi/bytevector.html")
  (description "Additional bytevector utilities.")
  (test "run-tests.scm"))
