(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib array-interpolate))
    (path "slib/array-interpolate.sld")
    (depends
      (scheme base)
      (slib array-for-each)
      (slib subarray)
      (srfi 63)))
  (manual "slib-array-interpolate.html")
  (description "Interpolated array access"))
