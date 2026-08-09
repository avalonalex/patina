(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs programs))
    (path "r6rs/programs.sld")
    (depends
      (scheme process-context)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs programs) to R7RS."))
