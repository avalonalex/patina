(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs r5rs))
    (path "r6rs/r5rs.sld")
    (depends
      (scheme r5rs)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs r5rs) to R7RS."))
