(package
  (authors "Retropikzel")
  (version "0.2.4")
  (library
    (name
      (srfi 170))
    (path "srfi/170.sld")
    (foreign-depends)
    (depends
      (scheme base)
      (scheme char)
      (scheme write)
      (scheme file)
      (scheme process-context)
      (foreign c)
      (srfi 19)))
  (manual "srfi/170/README.html")
  (description "Implementation of SRFI 170 - POSIX API using (foreign c)"))
