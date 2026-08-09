(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi term edit-line))
    (path "chibi/term/edit-line.sld")
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
          (chibi stty)))
      (chicken
        (depends stty))
      (else
        (depends)))
    (cond-expand
      (chibi
        (depends
          (chibi)
          (chibi ast)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (scheme write)))
  (manual "chibi/term/edit-line.html"))
