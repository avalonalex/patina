(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib resene))
    (path "slib/resene.sld")
    (depends
      (scheme base)
      (scheme char)
      (slib color)
      (srfi 69)))
  (manual "slib-resene.html")
  (description "Resene Color System"))
