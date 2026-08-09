(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs unicode))
    (path "r6rs/unicode.sld")
    (cond-expand
      ((and (library (scheme char)) (library (rnrs unicode)) (not (library (r6rs no-rnrs))))
        (depends
          (scheme char)
          (rnrs unicode)))
      ((library (scheme char))
        (depends
          (scheme char)
          (r6rs unicode-reference unicode1)
          (r6rs unicode-reference unicode3)
          (r6rs unicode-reference unicode4)))
      (else
        (depends
          (r6rs unicode-reference unicode1)
          (r6rs unicode-reference unicode3)
          (r6rs unicode-reference unicode4))))
    (depends))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs unicode) to R7RS."))
