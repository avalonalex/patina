(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib common))
    (path "slib/common.sld")
    (cond-expand
      (kawa
        (depends
          (kawa lib system)))
      (larceny
        (depends
          (primitives system)))
      ((library (chibi process))
        (depends
          (chibi process)))
      ((library (sagittarius process))
        (depends
          (sagittarius process)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme file)
      (scheme write)))
  (manual "slib-common.html")
  (description "SLIB core functions"))
