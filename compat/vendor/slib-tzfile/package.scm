(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib tzfile))
    (path "slib/tzfile.sld")
    (depends
      (scheme base)
      (scheme file)
      (slib byte)
      (slib common)))
  (manual "slib-time.html")
  (description "Read sysV style (binary) timezone file"))
