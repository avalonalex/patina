(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib byte))
    (path "slib/byte.sld")
    (depends
      (scheme base)
      (srfi 63)))
  (manual "slib-byte.html")
  (description "Arrays of small integers, not necessarily chars"))
