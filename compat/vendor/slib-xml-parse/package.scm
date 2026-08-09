(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib xml-parse))
    (path "slib/xml-parse.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)
      (slib common)
      (slib rev2-procedures)
      (slib string-search)
      (srfi 1)))
  (manual "slib-xml-parse.html")
  (description "XML parsing and conversion to SXML"))
