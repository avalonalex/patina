(package
  (maintainers "Retropikzel")
  (version "0.1.2")
  (library
    (name
      (srfi 11))
    (path "srfi/11.sld")
    (foreign-depends)
    (cond-expand
      (stklos
        (depends
          (scheme base)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      (stklos
        (depends))
      (else
        (depends)))
    (depends))
  (manual "README.html")
  (description "SRFI-11"))
