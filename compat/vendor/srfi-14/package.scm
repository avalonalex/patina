(package
  (maintainers "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 14))
    (path "srfi/14.sld")
    (foreign-depends)
    (cond-expand
      (mosh
        (depends
          (srfi :14 char-sets)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (scheme write)
      (srfi 60)))
  (manual "README.html")
  (description "SRFI-14"))
