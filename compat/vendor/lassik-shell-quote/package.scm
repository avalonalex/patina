(package
  (maintainers "Lassi Kortela <lassi@lassi.io>")
  (authors "Lassi Kortela")
  (version "0.2")
  (license ISC)
  (library
    (name
      (lassik shell-quote))
    (path "lassik/shell-quote.sld")
    (depends
      (scheme base)
      (scheme char)
      (chibi match)))
  (description "Scheme DSL to build shell command lines")
  (test "lassik/shell-quote-test.scm")
  (test-depends
    (scheme base)
    (scheme write)
    (lassik shell-quote)))
