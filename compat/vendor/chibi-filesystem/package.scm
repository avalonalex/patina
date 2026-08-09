(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi filesystem))
    (path "chibi/filesystem.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (chibi string)))
      (chicken
        (depends
          (scheme base)
          (srfi 1)
          (chicken)
          (posix)
          (chibi string)))
      (sagittarius
        (depends
          (scheme base)
          (sagittarius))))
    (depends))
  (library
    (name
      (chibi filesystem-test))
    (path "chibi/filesystem-test.sld")
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
      (scheme file)
      (scheme write)
      (chibi filesystem)
      (chibi test))
    (use-for test))
  (manual "chibi/filesystem.html")
  (description "Interface to the filesystem and file descriptor objects.")
  (test "run-tests.scm"))
