(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib pprint-file))
    (path "slib/pprint-file.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme file)
      (scheme read)
      (scheme write)
      (slib common)
      (slib pretty-print)))
  (manual "slib-pprint-file.html")
  (description "Pretty print a Scheme file"))
