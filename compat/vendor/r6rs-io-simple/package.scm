(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "Taylan Ulrich Bayırlı/Kammer <taylanbayirli@gmail.com>")
  (version "0.0.1")
  (license mit)
  (library
    (name
      (r6rs io simple))
    (path "r6rs/io/simple.sld")
    (depends
      (scheme base)
      (scheme file)
      (scheme read)
      (scheme write)))
  (manual "r6rsDoc.html")
  (description "Port of (rnrs io simple) to R7RS."))
