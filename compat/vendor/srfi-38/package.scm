(package
  (maintainers "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 38))
    (path "srfi/38.sld")
    (foreign-depends)
    (cond-expand
      (chibi
        (depends
          (chibi)
          (srfi 69)
          (chibi ast)))
      (else
        (depends
          (scheme base)
          (scheme char)
          (scheme cxr)
          (scheme write))))
    (cond-expand
      (chibi
        (depends))
      (else
        (depends)))
    (cond-expand
      (chibi
        (depends))
      (else
        (depends)))
    (depends))
  (manual "README.html")
  (description "SRFI-38"))
