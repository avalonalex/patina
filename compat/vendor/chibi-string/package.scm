(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi string))
    (path "chibi/string.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (chibi ast)
          (chibi char-set base)))
      (else
        (depends
          (scheme base)
          (scheme char)
          (srfi 14)
          (srfi 1))))
    (cond-expand
      (chibi
        (depends))
      ((library (srfi 13))
        (depends
          (srfi 13)))
      (else
        (depends)))
    (depends))
  (library
    (name
      (chibi string-test))
    (path "chibi/string-test.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (chibi test)
      (chibi string))
    (use-for test))
  (manual "chibi/string.html")
  (description "A cursor-oriented string library.")
  (test "run-tests.scm"))
