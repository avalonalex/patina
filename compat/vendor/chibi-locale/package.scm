(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi locale))
    (path "chibi/locale.sld")
    (cond-expand
      ((library (srfi 130))
        (depends
          (srfi 130)))
      (else
        (depends)))
    (depends
      (scheme base)))
  (library
    (name
      (chibi locale-test))
    (path "chibi/locale-test.sld")
    (depends
      (scheme base)
      (chibi locale)
      (chibi test))
    (use-for test))
  (manual "chibi/locale.html")
  (description "A lightweight library for representing locale information and serializing to and from strings.")
  (test "run-tests.scm"))
