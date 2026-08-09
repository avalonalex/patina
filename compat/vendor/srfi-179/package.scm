(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.10.0")
  (license bsd)
  (library
    (name
      (srfi 179))
    (path "srfi/179.sld")
    (depends
      (scheme base)
      (scheme list)
      (scheme vector)
      (scheme sort)
      (srfi 160 base)
      (srfi 179 base)
      (chibi assert)))
  (library
    (name
      (srfi 179 base))
    (path "srfi/179/base.sld")
    (depends
      (scheme base)
      (scheme list)
      (scheme vector)
      (chibi assert)))
  (library
    (name
      (srfi 179 test))
    (path "srfi/179/test.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (scheme complex)
      (scheme file)
      (scheme list)
      (scheme read)
      (scheme sort)
      (scheme vector)
      (scheme write)
      (chibi test)
      (srfi 27)
      (srfi 143)
      (srfi 144)
      (srfi 160 base)
      (srfi 179))
    (use-for test))
  (manual "https://srfi.schemers.org/srfi-179/srfi-179.html")
  (test "run-tests.scm"))
