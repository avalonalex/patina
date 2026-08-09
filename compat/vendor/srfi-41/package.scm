(package
  (maintainers "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 41))
    (path "srfi/41.sld")
    (foreign-depends)
    (cond-expand
      ((or cyclone mit-scheme)
        (depends))
      (stklos
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)))
  (manual "README.html")
  (description "SRFI-41"))
