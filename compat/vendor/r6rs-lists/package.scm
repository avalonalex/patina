(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs lists))
    (path "r6rs/lists.sld")
    (cond-expand
      ((and (library (rnrs lists)) (not (library (r6rs no-rnrs))))
        (depends
          (rnrs lists)))
      (else
        (depends
          (scheme base)
          (scheme case-lambda))))
    (depends))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs lists) to R7RS."))
