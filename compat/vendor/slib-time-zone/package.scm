(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib time-zone))
    (path "slib/time-zone.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)
      (scheme file)
      (slib common)
      (slib scanf)
      (slib time-core)
      (slib tzfile)))
  (manual "slib-time.html")
  (description "Compute timezones and DST from TZ environment variable"))
