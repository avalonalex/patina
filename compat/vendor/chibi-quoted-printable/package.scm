(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi quoted-printable))
    (path "chibi/quoted-printable.sld")
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
      (scheme base)))
  (library
    (name
      (chibi quoted-printable-test))
    (path "chibi/quoted-printable-test.sld")
    (depends
      (scheme base)
      (chibi quoted-printable)
      (chibi string)
      (chibi test))
    (use-for test))
  (manual "chibi/quoted-printable.html")
  (description "RFC 2045 quoted printable encoding and decoding utilities.")
  (test "run-tests.scm"))
