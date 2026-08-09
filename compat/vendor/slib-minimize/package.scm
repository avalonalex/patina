(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Lars Arvestad")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib minimize))
    (path "slib/minimize.sld")
    (depends
      (scheme base)
      (scheme inexact)
      (slib common)))
  (manual "slib-minimize.html")
  (description "Finds minimum value of a function"))
