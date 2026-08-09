(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs enums))
    (path "r6rs/enums.sld")
    (cond-expand
      ((and (library (rnrs enums)) (not (library (r6rs no-rnrs))))
        (depends
          (rnrs enums)))
      (else
        (depends
          (scheme base)
          (r6rs lists)
          (r6rs sorting))))
    (depends))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs enums) to R7RS."))
