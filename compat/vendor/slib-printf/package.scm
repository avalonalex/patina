(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer and Radey Shouman")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib printf))
    (path "slib/printf.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme complex)
      (scheme write)
      (slib generic-write)))
  (manual "slib-printf.html")
  (description "Implementation of standard C functions"))
