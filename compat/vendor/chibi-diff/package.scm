(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.1.3")
  (license bsd)
  (library
    (name
      (chibi diff))
    (path "chibi/diff.sld")
    (cond-expand
      (chibi
        (depends
          (chibi io)))
      (else
        (depends)))
    (depends
      (scheme base)
      (srfi 1)
      (chibi optional)
      (chibi term ansi)))
  (library
    (name
      (chibi diff-test))
    (path "chibi/diff-test.sld")
    (cond-expand
      (chibi
        (depends
          (chibi test)))
      (else
        (depends
          (scheme write))))
    (depends
      (scheme base)
      (chibi diff))
    (use-for test))
  (manual "chibi/diff.html")
  (test "run-tests.scm"))
