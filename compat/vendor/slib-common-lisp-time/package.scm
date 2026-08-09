(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib common-lisp-time))
    (path "slib/common-lisp-time.sld")
    (depends
      (scheme base)
      (scheme process-context)
      (slib common)
      (slib time-core)
      (slib time-zone)))
  (manual "slib-time.html")
  (description "Common-Lisp time conversion routines"))
