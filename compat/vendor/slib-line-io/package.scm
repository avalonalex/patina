(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib line-io))
    (path "slib/line-io.sld")
    (depends
      (scheme base)
      (scheme case-lambda)
      (scheme file)
      (scheme write)
      (slib common)
      (slib filename)))
  (manual "slib-line-io.html")
  (description "Line oriented input/output functions"))
