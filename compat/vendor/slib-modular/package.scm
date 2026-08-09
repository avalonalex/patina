(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib modular))
    (path "slib/modular.sld")
    (depends
      (scheme base)
      (slib common)))
  (manual "slib-modular.html")
  (description "Modular fixnum arithmetic"))
