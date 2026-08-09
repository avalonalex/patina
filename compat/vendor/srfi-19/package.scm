(package
  (maintainers "Retropikzel")
  (version "1.0.2")
  (library
    (name
      (srfi 19))
    (path "srfi/19.sld")
    (foreign-depends)
    (cond-expand
      (chicken
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme write)
      (scheme read)
      (scheme file)
      (scheme time)
      (scheme char)
      (scheme cxr)
      (srfi 8)))
  (manual "README.html")
  (description "SRFI-19"))
