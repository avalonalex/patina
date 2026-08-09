(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi voting))
    (path "chibi/voting.sld")
    (depends
      (scheme base)
      (scheme hash-table)
      (scheme list)
      (scheme sort)
      (scheme vector)
      (srfi 227)))
  (library
    (name
      (chibi voting-test))
    (path "chibi/voting-test.sld")
    (depends
      (scheme base)
      (chibi voting)
      (chibi test))
    (use-for test))
  (manual "chibi/voting.html")
  (description "Preferential voting utilities to help come to reasonable decisions when there are more than 2 options.")
  (test "run-tests.scm"))
