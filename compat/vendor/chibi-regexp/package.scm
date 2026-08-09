(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi regexp))
    (path "chibi/regexp.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (scheme char)
          (srfi 9)
          (chibi char-set)
          (chibi char-set full)
          (chibi char-set ascii)))
      (else
        (depends
          (scheme base)
          (scheme char)
          (srfi 1)
          (srfi 14))))
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (cond-expand
      (chibi
        (depends))
      (else
        (depends)))
    (depends
      (chibi char-set boundary)
      (srfi 69)))
  (library
    (name
      (chibi regexp pcre))
    (path "chibi/regexp/pcre.sld")
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)
      (srfi 1)
      (chibi string)
      (chibi regexp)))
  (library
    (name
      (chibi regexp-test))
    (path "chibi/regexp-test.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme file)
      (scheme write)
      (chibi regexp)
      (chibi regexp pcre)
      (chibi string)
      (chibi match)
      (chibi test))
    (use-for test))
  (manual "chibi/regexp.html" "chibi/regexp/pcre.html")
  (description "A regular expression engine implementing SRFI 115 using a non-backtracking Thompson NFA algorithm.")
  (test "run-tests.scm"))
