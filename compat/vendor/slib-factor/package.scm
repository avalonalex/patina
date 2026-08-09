(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib factor))
    (path "slib/factor.sld")
    (depends
      (scheme base)
      (slib common)
      (slib modular)
      (srfi 27)))
  (manual "slib-factor.html")
  (description "Factorization, prime test and generation"))
