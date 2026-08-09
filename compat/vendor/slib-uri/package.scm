(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib uri))
    (path "slib/uri.sld")
    (depends
      (scheme base)
      (scheme case-lambda)
      (scheme char)
      (scheme cxr)
      (slib coerce)
      (slib common)
      (slib directory)
      (slib printf)
      (slib scanf)
      (slib string-case)
      (slib string-search)
      (srfi 1)))
  (manual "slib-uri.html")
  (description "Construct and decode Uniform Resource Identifiers"))
