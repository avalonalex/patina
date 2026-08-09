(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib daylight))
    (path "slib/daylight.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (scheme inexact)
      (slib color-space)))
  (manual "slib-daylight.html")
  (description "Model of sun and sky colors"))
