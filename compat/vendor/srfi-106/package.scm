(package
  (authors "Retropikzel")
  (version "0.1.0")
  (library
    (name
      (srfi 106))
    (path "srfi/106.sld")
    (foreign-depends)
    (depends
      (scheme base)
      (scheme write)
      (scheme process-context)
      (foreign c)))
  (manual "srfi/106/README.html")
  (description "SRFI-106: Basic socket interface"))
