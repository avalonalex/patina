(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi pathname))
    (path "chibi/pathname.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base))))
    (depends
      (chibi string)))
  (library
    (name
      (chibi pathname-test))
    (path "chibi/pathname-test.sld")
    (depends
      (scheme base)
      (chibi pathname)
      (chibi test))
    (use-for test))
  (manual "chibi/pathname.html")
  (description "A general, non-filesystem-specific pathname library.")
  (test "run-tests.scm"))
