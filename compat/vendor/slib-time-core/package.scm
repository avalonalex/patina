(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib time-core))
    (path "slib/time-core.sld")
    (depends
      (scheme base)
      (scheme time)))
  (manual "slib-time.html")
  (description "Core time conversion routines"))
