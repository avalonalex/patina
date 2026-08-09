(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.5.7")
  (license bsd)
  (library
    (name
      (chibi html-parser))
    (path "chibi/html-parser.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)
      (scheme write)))
  (manual "chibi/html-parser.html")
  (description "A permissive HTML parser supporting scalable streaming with a tree folding interface."))
