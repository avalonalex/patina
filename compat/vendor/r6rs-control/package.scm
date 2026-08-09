(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs control))
    (path "r6rs/control.sld")
    (depends
      (scheme base)
      (scheme case-lambda)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs control) to R7RS."))
