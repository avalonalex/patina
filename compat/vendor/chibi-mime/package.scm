(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi mime))
    (path "chibi/mime.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme write)
      (chibi base64)
      (chibi quoted-printable)
      (chibi string)))
  (library
    (name
      (chibi mime-test))
    (path "chibi/mime-test.sld")
    (depends
      (scheme base)
      (chibi mime)
      (chibi string)
      (chibi test))
    (use-for test))
  (manual "chibi/mime.html")
  (description "A library to parse MIME headers and bodies into SXML.")
  (test "run-tests.scm"))
