(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs arithmetic fixnums))
    (path "r6rs/arithmetic/fixnums.sld")
    (cond-expand
      ((and (library (rnrs arithmetic fixnums)) (not (library (r6rs no-rnrs))))
        (depends
          (rnrs arithmetic fixnums)))
      ((and sagittarius x86_64)
        (depends))
      ((and larceny ilp32)
        (depends))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme case-lambda)
      (r6rs base)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs arithmetic fixnums) to R7RS."))
