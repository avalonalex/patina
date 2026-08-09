(package
  (maintainers "Arvydas Silanskas <nma.arvydas.silanskas@gmail.com>")
  (authors "Arvydas Silanskas")
  (version "1.0.0")
  (license mit)
  (library
    (name
      (arvyy interface))
    (path "arvyy/interface.sld")
    (depends
      (scheme base)))
  (library
    (name
      (arvyy interface-test))
    (path "arvyy/interface-test.sld")
    (cond-expand
      ((library (srfi 64))
        (depends
          (srfi 64)))
      (chibi
        (depends
          (chibi test))))
    (depends
      (arvyy interface)
      (scheme base))
    (use-for test))
  (manual "readme.html")
  (description "Interface abstraction for a set of functions")
  (test "run-tests.scm"))
