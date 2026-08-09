(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs bytevectors))
    (path "r6rs/bytevectors.sld")
    (cond-expand
      ((and (library (rnrs bytevectors)) (not (library (r6rs no-rnrs))))
        (depends
          (scheme base)
          (rnrs bytevectors)
          (rnrs bytevectors)))
      (else
        (depends
          (scheme base)
          (scheme inexact))))
    (cond-expand
      ((and (library (rnrs bytevectors)) (not (library (r6rs no-rnrs))))
        (depends))
      (else
        (depends)))
    (depends))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs bytevectors) to R7RS."))
