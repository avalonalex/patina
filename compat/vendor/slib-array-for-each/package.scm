(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib array-for-each))
    (path "slib/array-for-each.sld")
    (depends
      (scheme base)
      (slib common)
      (srfi 1)
      (srfi 63)))
  (manual "slib-array-foreach.html")
  (description "Applicative routines for arrays/matrices"))
