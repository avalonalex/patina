(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.1.3")
  (license bsd)
  (library
    (name
      (chibi optional))
    (path "chibi/optional.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base))))
    (depends))
  (library
    (name
      (chibi optional-test))
    (path "chibi/optional-test.sld")
    (cond-expand
      (chibi
        (depends
          (chibi test)))
      (else
        (depends
          (scheme write))))
    (depends
      (scheme base)
      (chibi optional))
    (use-for test))
  (manual "chibi/optional.html")
  (description "Syntax to support optional and named keyword arguments.")
  (test "run-tests.scm"))
