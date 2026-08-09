(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib coerce))
    (path "slib/coerce.sld")
    (depends
      (scheme base)
      (srfi 63)))
  (manual "slib-coerce.html")
  (description "Implementation of COMMON-LISP COERCE and TYPE-OF"))
