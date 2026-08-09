(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi tar))
    (path "chibi/tar.sld")
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
        (depends
          (chibi system)))
      (chicken
        (depends posix)))
    (depends
      (scheme base)
      (scheme file)
      (scheme time)
      (srfi 1)
      (scheme write)
      (chibi string)
      (chibi binary-record)
      (chibi pathname)
      (chibi filesystem)))
  (library
    (name
      (chibi tar-test))
    (path "chibi/tar-test.sld")
    (depends
      (scheme base)
      (chibi tar)
      (chibi test))
    (use-for test))
  (manual "chibi/tar.html")
  (test "run-tests.scm"))
