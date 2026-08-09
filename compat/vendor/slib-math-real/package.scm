(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs-1")
  (library
    (name
      (slib math-real))
    (path "slib/math-real.sld")
    (depends
      (scheme base)
      (scheme complex)
      (scheme inexact)
      (slib common)))
  (manual "slib-math-real.html")
  (description "Mathematical functions restricted to real numbers"))
