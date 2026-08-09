(package
  (authors "Retropikzel")
  (version "1.0.8")
  (library
    (name
      (retropikzel pstk))
    (path "retropikzel/pstk.sld")
    (foreign-depends)
    (cond-expand
      (kawa
        (depends
          (kawa base)))
      (srfi-88
        (depends
          (srfi 88)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme cxr)
      (scheme read)
      (scheme file)
      (scheme write)
      (scheme process-context)
      (retropikzel named-pipes)
      (retropikzel system)
      (srfi 170)))
  (manual "retropikzel/pstk/README.html")
  (description "Use Tk GUI from Scheme"))
