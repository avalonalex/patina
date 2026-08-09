(package
  (maintainers "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 26))
    (path "srfi/26.sld")
    (foreign-depends)
    (cond-expand
      (skint
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)))
  (manual "README.html")
  (description "SRFI-26"))
