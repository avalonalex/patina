(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs eval))
    (path "r6rs/eval.sld")
    (depends
      (scheme eval)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs eval) to R7RS."))
