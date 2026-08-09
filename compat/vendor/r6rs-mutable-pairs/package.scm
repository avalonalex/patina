(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs mutable-pairs))
    (path "r6rs/mutable-pairs.sld")
    (depends
      (scheme base)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs mutable-pairs) to R7RS."))
