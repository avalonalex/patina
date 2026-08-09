(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.3")
  (license bsd)
  (library
    (name
      (chibi irregex))
    (path "irregex.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)))
  (manual "irregex.html")
  (description "A portable and efficient R[4567]RS implementation of regular expressions, supporting both POSIX syntax with various (irregular) PCRE extensions, as well as SCSH's SRE syntax."))
