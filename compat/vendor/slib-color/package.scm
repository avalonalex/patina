(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib color))
    (path "slib/color.sld")
    (depends
      (scheme base)
      (slib color-space)
      (slib printf)
      (slib scanf)
      (slib string-case)))
  (manual "slib-color.html")
  (description "Color data type"))
