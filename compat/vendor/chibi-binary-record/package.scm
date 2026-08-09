(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi binary-record))
    (path "chibi/binary-record.sld")
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
      ((library (srfi 130))
        (depends
          (srfi 130)))
      (else
        (depends
          (srfi 13))))
    (cond-expand
      (chicken
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)
      (srfi 1)))
  (manual "chibi/binary-record.html"))
