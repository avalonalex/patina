(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.1")
  (license public-domain)
  (library
    (name
      (chibi match))
    (path "chibi/match.sld")
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
      (chibi match-test))
    (path "chibi/match-test.sld")
    (cond-expand
      (chibi
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)
      (chibi match)
      (chibi test))
    (use-for test))
  (manual "chibi/match.html")
  (description "A portable hygienic pattern matcher.")
  (test "run-tests.scm"))
