(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi temp-file))
    (path "chibi/temp-file.sld")
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (cond-expand
      (chibi
        (depends
          (chibi process)))
      (chicken
        (depends
          (posix)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme time)
      (chibi filesystem)
      (chibi pathname)))
  (manual "chibi/temp-file.html"))
