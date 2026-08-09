(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib scanf))
    (path "slib/scanf.sld")
    (depends
      (scheme base)
      (scheme case-lambda)
      (scheme char)
      (scheme cxr)
      (slib common)
      (slib string-port)))
  (manual "slib-scanf.html")
  (description "Implementation of POSIX-style formatted input"))
