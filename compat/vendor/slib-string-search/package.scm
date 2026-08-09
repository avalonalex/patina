(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Oleg Kiselyov" " Aubrey Jaffer and Steve VanDevender")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib string-search))
    (path "slib/string-search.sld")
    (cond-expand
      ((library (srfi 13))
        (depends
          (srfi 13)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (slib alist)))
  (manual "slib-strsearch.html")
  (description "Functions for working with and searching within strings"))
