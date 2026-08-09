(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs hashtables))
    (path "r6rs/hashtables.sld")
    (cond-expand
      ((library (rnrs hashtables))
        (depends))
      ((library (scheme inexact))
        (depends
          (scheme inexact)))
      (else
        (depends)))
    (cond-expand
      ((and (library (rnrs hashtables)) (not (library (r6rs no-rnrs))))
        (depends))
      ((library (scheme complex))
        (depends
          (scheme complex)))
      (else
        (depends)))
    (cond-expand
      ((and (library (rnrs hashtables)) (not (library (r6rs no-rnrs))))
        (depends
          (rnrs hashtables)))
      ((library (srfi 69 basic-hash-tables))
        (depends
          (srfi 69 basic-hash-tables)))
      ((library (srfi 69))
        (depends
          (srfi 69)))
      ((library (srfi :69 basic-hash-tables))
        (depends
          (srfi :69 basic-hash-tables)))
      ((library (srfi :69))
        (depends
          (srfi :69)))
      ((library (scheme char))
        (depends
          (scheme char)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme cxr)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs hashtables) to R7RS."))
