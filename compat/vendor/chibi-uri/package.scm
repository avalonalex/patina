(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi uri))
    (path "chibi/uri.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (srfi 9)))
      (else
        (depends
          (scheme base)
          (scheme char))))
    (depends
      (chibi string)
      (chibi pathname)))
  (library
    (name
      (chibi uri-test))
    (path "chibi/uri-test.sld")
    (depends
      (scheme base)
      (chibi test)
      (chibi uri))
    (use-for test))
  (manual "chibi/uri.html")
  (description "Library for parsing and constructing URI objects.")
  (test "run-tests.scm"))
