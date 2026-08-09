(package
  (maintainers "Chris Oei <chris.oei@gmail.com>")
  (authors "Chris Oei <chris.oei@gmail.com>")
  (version "0.1.0")
  (license mit)
  (library
    (name
      (chrisoei cint))
    (path "cint.sld")
    (depends
      (scheme base)))
  (description "Compute cint coefficients")
  (test "cint-test.scm")
  (test-depends
    (scheme small)
    (srfi 144)
    (chibi test)
    (chrisoei cint)))
