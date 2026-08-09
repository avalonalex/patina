(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs mutable-strings))
    (path "r6rs/mutable-strings.sld")
    (depends
      (scheme base)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs mutable-strings) to R7RS."))
