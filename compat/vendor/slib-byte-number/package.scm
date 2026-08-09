(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib byte-number))
    (path "slib/byte-number.sld")
    (depends
      (scheme base)
      (scheme complex)
      (scheme inexact)
      (slib byte)
      (slib common)
      (srfi 60)))
  (manual "slib-byte-number.html")
  (description "Byte integer and IEEE floating-point conversions"))
