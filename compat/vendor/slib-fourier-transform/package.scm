(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib fourier-transform))
    (path "slib/fourier-transform.sld")
    (depends
      (scheme base)
      (scheme inexact)
      (slib subarray)
      (srfi 1)
      (srfi 60)
      (srfi 63)))
  (manual "slib-fourier.html")
  (description "Discrete Fourier Transform"))
