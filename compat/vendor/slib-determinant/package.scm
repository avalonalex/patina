(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib determinant))
    (path "slib/determinant.sld")
    (depends
      (scheme base)
      (srfi 63)))
  (manual "slib-determinant.html")
  (description "Matrix Algebra"))
