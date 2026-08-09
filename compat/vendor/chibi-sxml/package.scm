(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi sxml))
    (path "chibi/sxml.sld")
    (depends
      (scheme base)
      (scheme write)))
  (manual "chibi/sxml.html")
  (description "Utilities to convert sxml to xml or plain text."))
