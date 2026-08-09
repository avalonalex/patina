(package
  (maintainers "Retropikzel")
  (version "0.1.1")
  (library
    (name
      (srfi 42))
    (path "srfi/42.sld")
    (foreign-depends)
    (cond-expand
      (tr7
        (depends
          (scheme base)
          (scheme read)
          (scheme cxr)))
      (stklos
        (depends
          (scheme base)
          (scheme read)
          (scheme cxr)
          (scheme complex)
          (stklos)))
      (else
        (depends
          (scheme base)
          (scheme read)
          (scheme cxr)
          (scheme complex))))
    (cond-expand
      (stklos
        (depends))
      (else
        (depends)))
    (cond-expand
      (stklos
        (depends))
      (else
        (depends)))
    (cond-expand
      ((or chicken stklos)
        (depends))
      (else
        (depends)))
    (depends))
  (manual "README.html")
  (description "SRFI-42"))
