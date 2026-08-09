(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib pretty-print))
    (path "slib/pretty-print.sld")
    (depends
      (scheme base)
      (scheme write)
      (slib common)
      (slib generic-write)))
  (manual "slib-pretty-print.html")
  (description "Pretty printing"))
