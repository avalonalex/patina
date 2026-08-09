(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi monad environment))
    (path "chibi/monad/environment.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends)))
    (depends
      (scheme base)))
  (manual "chibi/monad/environment.html")
  (description "A Scheme take on the environment (reader) monad, focusing more on being efficient and convenient than pure."))
