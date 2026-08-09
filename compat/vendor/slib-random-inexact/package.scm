(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib random-inexact))
    (path "slib/random-inexact.sld")
    (depends
      (scheme base)
      (scheme inexact)
      (srfi 27)))
  (manual "slib-random-inexact.html")
  (description "Pseudo-Random inexact real numbers"))
