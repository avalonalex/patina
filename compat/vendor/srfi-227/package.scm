(package
  (maintainers "Retropikzel")
  (version "0.1.2")
  (library
    (name
      (srfi 227))
    (path "srfi/227.sld")
    (foreign-depends)
    (cond-expand
      (chicken
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme case-lambda)))
  (manual "README.html")
  (description "SRFI-227"))
