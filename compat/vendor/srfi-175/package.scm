(package
  (maintainers "Lassi Kortela <lassi@lassi.io>")
  (authors "Lassi Kortela")
  (version "1.1")
  (license MIT)
  (library
    (name
      (srfi 175))
    (path "srfi/175.sld")
    (depends
      (scheme base)))
  (description "SRFI 175: ASCII character library")
  (test "srfi/tests.scm")
  (test-depends
    (scheme base)
    (scheme file)
    (scheme read)
    (scheme write)
    (srfi 175)))
