(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.10.0")
  (license bsd)
  (library
    (name
      (chibi assert))
    (path "chibi/assert.sld")
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
      (chibi assert-test))
    (path "chibi/assert-test.sld")
    (depends
      (chibi)
      (chibi assert)
      (chibi test))
    (use-for test))
  (manual "chibi/assert.html")
  (description "A nice assert macro.")
  (test "run-tests.scm"))
