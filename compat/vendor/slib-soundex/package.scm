(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "jjb and L.J.Buitinck")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib soundex))
    (path "slib/soundex.sld")
    (depends
      (scheme base)
      (scheme char)
      (srfi 1)))
  (manual "slib-soundex.html")
  (description "Original Soundex algorithm"))
