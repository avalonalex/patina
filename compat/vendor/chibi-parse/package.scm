(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi parse))
    (path "chibi/parse.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (chibi char-set)
          (srfi 9)))
      (else
        (depends
          (scheme base)
          (scheme char)
          (scheme file)
          (srfi 14))))
    (depends))
  (library
    (name
      (chibi parse common))
    (path "chibi/parse/common.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base)
          (scheme char))))
    (depends
      (chibi parse)))
  (library
    (name
      (chibi parse-test))
    (path "chibi/parse-test.sld")
    (cond-expand
      (chibi
        (depends
          (chibi char-set)
          (chibi char-set ascii)))
      (else
        (depends
          (srfi 14))))
    (depends
      (scheme base)
      (scheme char)
      (chibi test)
      (chibi parse)
      (chibi parse common))
    (use-for test))
  (manual "chibi/parse.html" "chibi/parse/common.html")
  (description "A parser combinator library with optional memoization and convenient syntax.")
  (test "run-tests.scm"))
