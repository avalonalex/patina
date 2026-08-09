(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib posix-time))
    (path "slib/posix-time.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme process-context)
      (slib time-core)
      (slib time-zone)))
  (manual "slib-time.html")
  (description "POSIX time conversion routines"))
