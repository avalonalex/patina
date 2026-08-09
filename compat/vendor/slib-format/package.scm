(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Dirk Lutzebaeck" " Ken Dickey" " Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib format))
    (path "slib/format.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme complex)
      (scheme cxr)
      (scheme write)
      (slib common)
      (slib pretty-print)
      (slib string-case)
      (slib string-port)))
  (manual "slib-format.html")
  (description "Common LISP text output formatter"))
