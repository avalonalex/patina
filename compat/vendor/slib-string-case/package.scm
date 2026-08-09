(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Dirk Lutzebaeck" " Ken Dickey and Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib string-case))
    (path "slib/string-case.sld")
    (cond-expand
      ((library (srfi 13))
        (depends
          (srfi 13)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (slib common)))
  (manual "slib-string-case.html")
  (description "String casing functions"))
