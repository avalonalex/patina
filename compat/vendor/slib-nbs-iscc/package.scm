(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib nbs-iscc))
    (path "slib/nbs-iscc.sld")
    (depends
      (scheme base)
      (scheme char)
      (slib color)
      (srfi 69)))
  (manual "slib-nbs-iscc.html")
  (description "NBS/ISCC Color System"))
