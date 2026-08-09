(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs base))
    (path "r6rs/base.sld")
    (cond-expand
      ((and (or (library (rnrs base)) larceny) (not (library (r6rs no-rnrs))))
        (depends
          (rnrs base)))
      (else
        (depends
          (scheme base)
          (scheme cxr))))
    (cond-expand
      ((and (or (library (rnrs base)) larceny) (not (library (r6rs no-rnrs))))
        (depends))
      ((library (scheme inexact))
        (depends
          (scheme inexact)))
      (else
        (depends)))
    (cond-expand
      ((and (or (library (rnrs base)) larceny) (not (library (r6rs no-rnrs))))
        (depends))
      ((library (scheme complex))
        (depends
          (scheme complex)))
      (else
        (depends)))
    (cond-expand
      ((and (or (library (rnrs base)) larceny) (not (library (r6rs no-rnrs))))
        (depends))
      (else
        (depends)))
    (depends))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs base) to R7RS."))
