(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi net smtp))
    (path "chibi/net/smtp.sld")
    (cond-expand
      ((library (chibi ssl))
        (depends
          (chibi ssl)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme file)
      (scheme char)
      (srfi 1)
      (srfi 2)
      (srfi 69)
      (chibi net)
      (chibi net dns)
      (chibi optional)
      (chibi string)
      (chibi regexp)
      (chibi system)
      (chibi process)
      (chibi pathname)
      (chibi time)
      (chibi mime)
      (chibi base64)
      (chibi quoted-printable)))
  (library
    (name
      (chibi net smtp-test))
    (path "chibi/net/smtp-test.sld")
    (depends
      (scheme base)
      (scheme char)
      (chibi string)
      (chibi test)
      (chibi net smtp))
    (use-for test))
  (manual "chibi/net/smtp.html")
  (description "Easy mail interface.")
  (test "run-tests.scm"))
