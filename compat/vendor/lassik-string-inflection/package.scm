(package
  (maintainers "Lassi Kortela <lassi@lassi.io>")
  (authors "Lassi Kortela")
  (version "0.2")
  (license ISC)
  (library
    (name
      (lassik string-inflection))
    (path "lassik/string-inflection.sld")
    (depends
      (scheme base)
      (scheme char)))
  (description "lisp-case under_score CapsUpper capsLower")
  (test "lassik/string-inflection-test.scm")
  (test-depends
    (scheme base)
    (srfi 64)
    (lassik string-inflection)))
