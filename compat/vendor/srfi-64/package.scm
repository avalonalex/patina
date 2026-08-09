(package
  (maintainers "Retropikzel")
  (version "0.2.1")
  (library
    (name
      (srfi 64))
    (path "srfi/64.sld")
    (foreign-depends)
    (cond-expand
      ((library (scheme complex))
        (depends
          (scheme base)
          (scheme case-lambda)
          (scheme complex)
          (scheme eval)
          (scheme file)
          (scheme process-context)
          (scheme read)
          (scheme write)))
      (else
        (depends
          (scheme base)
          (scheme case-lambda)
          (scheme eval)
          (scheme file)
          (scheme process-context)
          (scheme read)
          (scheme write))))
    (cond-expand
      (chicken
        (depends))
      (stklos
        (depends))
      (meevax
        (depends))
      (else
        (depends)))
    (cond-expand
      (chicken-5
        (depends))
      (else
        (depends)))
    (cond-expand
      (racket
        (depends))
      (else
        (depends)))
    (depends))
  (manual "README.html")
  (description "SRFI-64")
  (test "test.scm")
  (test-depends
    (scheme base)
    (scheme char)
    (scheme inexact)
    (scheme read)
    (scheme write)
    (scheme process-context)
    (scheme file)
    (scheme cxr)
    (srfi 64)))
