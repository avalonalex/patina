(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs unicode-reference unicode3))
    (path "r6rs/unicode-reference/unicode3.sld")
    (depends
      (scheme base)
      (r6rs unicode-reference unicode0)
      (r6rs unicode-reference unicode1)
      (r6rs unicode-reference unicode2)))
  (manual "r6rsDoc.html")
  (description "Helper library for (rnrs unicode)."))
