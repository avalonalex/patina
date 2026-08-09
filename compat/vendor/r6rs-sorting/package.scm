(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs sorting))
    (path "r6rs/sorting.sld")
    (cond-expand
      ((and (library (rnrs sorting)) (not (library (r6rs no-rnrs))))
        (depends
          (rnrs sorting)))
      (else
        (depends
          (scheme base))))
    (depends))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs sorting) to R7RS."))
