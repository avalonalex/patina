(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib directory))
    (path "slib/directory.sld")
    (cond-expand
      ((library (chibi filesystem))
        (depends
          (chibi filesystem)
          (chibi pathname)))
      (gauche
        (depends
          (file util)))
      (kawa
        (depends
          (kawa lib files)
          (kawa lib ports)
          (kawa base)))
      (larceny
        (depends
          (primitives current-directory list-directory)
          (srfi 59)))
      (sagittarius
        (depends
          (sagittarius)
          (util file)
          (srfi 1)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme case-lambda)
      (slib common)
      (slib filename)))
  (manual "slib-directory.html")
  (description "Directories"))
