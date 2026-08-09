(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib charplot))
    (path "slib/charplot.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (slib array-for-each)
      (slib common)
      (slib printf)
      (srfi 63)))
  (manual "slib-charplot.html")
  (description "Plotting histograms/graphs in characters"))
