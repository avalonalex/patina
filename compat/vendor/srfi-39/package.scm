(package
  (maintainers "Retropikzel")
  (version "2025.08.27")
  (library
    (name
      (srfi 39))
    (path "srfi/39.sld")
    (foreign-depends)
    (cond-expand
      (racket
        (depends
          (scheme base)))
      (tr7
        (depends
          (scheme base)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      (stklos
        (depends))
      (cyclone
        (depends))
      (mit
        (depends))
      (else
        (depends)))
    (cond-expand
      (racket
        (depends))
      (tr7
        (depends))
      (else
        (depends)))
    (depends))
  (manual "README.html")
  (description "SRFI-39"))
