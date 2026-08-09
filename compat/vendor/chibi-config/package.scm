(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi config))
    (path "chibi/config.sld")
    (cond-expand
      (chibi
        (depends
          (chibi filesystem)
          (chibi)
          (meta)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme read)
      (scheme write)
      (scheme file)
      (scheme time)
      (srfi 1)))
  (manual "chibi/config.html")
  (description "This is a library for unified configuration management."))
