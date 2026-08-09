(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi test))
    (path "chibi/test.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (chicken
        (depends
          (chicken)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme write)
      (scheme complex)
      (scheme process-context)
      (scheme time)
      (chibi diff)
      (chibi term ansi)))
  (manual "chibi/test.html")
  (description "Simple but extensible testing framework with advanced reporting."))
