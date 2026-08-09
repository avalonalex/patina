(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib rev2-procedures))
    (path "slib/rev2-procedures.sld")
    (depends
      (scheme base)
      (srfi 1)))
  (manual "slib-rev2.html")
  (description "Implementation of some R2RS procedures eliminated in subsequence versions"))
