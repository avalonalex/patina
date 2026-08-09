(package
  (maintainers "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 128))
    (path "srfi/128.sld")
    (foreign-depends)
    (cond-expand
      (tr7
        (depends
          (scheme base)
          (scheme case-lambda)
          (scheme char)
          (scheme inexact)))
      (else
        (depends
          (scheme base)
          (scheme case-lambda)
          (scheme char)
          (scheme inexact)
          (scheme complex))))
    (cond-expand
      (mit
        (depends))
      (else
        (depends)))
    (depends))
  (manual "README.html")
  (description "SRFI-128"))
