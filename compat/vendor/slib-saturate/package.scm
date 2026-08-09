(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib saturate))
    (path "slib/saturate.sld")
    (depends
      (scheme base)
      (scheme char)
      (slib color)
      (srfi 69)))
  (manual "slib-saturate.html")
  (description "Saturated Color Dictionary"))
