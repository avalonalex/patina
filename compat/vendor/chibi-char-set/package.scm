(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi char-set))
    (path "chibi/char-set.sld")
    (depends
      (chibi char-set base)
      (chibi char-set extras)))
  (library
    (name
      (chibi char-set base))
    (path "chibi/char-set/base.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base))))
    (depends
      (chibi iset base)))
  (library
    (name
      (chibi char-set extras))
    (path "chibi/char-set/extras.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base))))
    (depends
      (chibi iset)
      (chibi char-set base)))
  (manual "chibi/char-set.html")
  (description "A minimal character set library."))
