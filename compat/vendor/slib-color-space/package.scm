(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib color-space))
    (path "slib/color-space.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (scheme file)
      (scheme inexact)
      (scheme read)
      (srfi 60)
      (srfi 95)))
  (manual "slib-color-space.html")
  (description "Color-space conversions"))
