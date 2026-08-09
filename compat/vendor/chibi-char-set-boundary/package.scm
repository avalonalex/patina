(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license public-domain)
  (library
    (name
      (chibi char-set boundary))
    (path "chibi/char-set/boundary.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      ((library (chibi char-set))
        (depends
          (chibi char-set)))
      (else
        (depends
          (srfi 14))))
    (depends))
  (manual "chibi/char-set/boundary.html")
  (description "Char-sets used for TR29 word boundaries."))
