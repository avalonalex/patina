(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs exceptions))
    (path "r6rs/exceptions.sld")
    (depends
      (scheme base)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs exceptions) to R7RS."))
