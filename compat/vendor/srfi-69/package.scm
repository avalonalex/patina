(package
  (maintainers "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 69))
    (path "srfi/69.sld")
    (foreign-depends)
    (cond-expand
      (tr7
        (depends
          (scheme base)
          (scheme char)
          (scheme cxr)))
      (else
        (depends
          (scheme base)
          (scheme char)
          (scheme complex)
          (scheme cxr))))
    (depends))
  (manual "README.html")
  (description "SRFI-69"))
