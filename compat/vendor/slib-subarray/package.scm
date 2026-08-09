(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer and Radey Shouman")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib subarray))
    (path "slib/subarray.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (srfi 63)))
  (manual "slib-subarray.html")
  (description "Accessing parts of arrays"))
