(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi base64))
    (path "chibi/base64.sld")
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
          (chibi io)))
      (else
        (depends)))
    (depends
      (scheme base)
      (chibi string)))
  (library
    (name
      (chibi base64-test))
    (path "chibi/base64-test.sld")
    (depends
      (scheme base)
      (chibi base64)
      (chibi string)
      (chibi test))
    (use-for test))
  (manual "chibi/base64.html")
  (description "RFC 3548 base64 encoding and decoding utilities.")
  (test "run-tests.scm"))
