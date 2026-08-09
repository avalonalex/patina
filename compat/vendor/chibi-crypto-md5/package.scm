(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi crypto md5))
    (path "chibi/crypto/md5.sld")
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (scheme base)
      (chibi bytevector)))
  (library
    (name
      (chibi crypto md5-test))
    (path "chibi/crypto/md5-test.sld")
    (depends
      (scheme base)
      (chibi crypto md5)
      (chibi test))
    (use-for test))
  (manual "chibi/crypto/md5.html")
  (description "Implementation of the MD5 (Message Digest) cryptographic hash.")
  (test "run-tests.scm"))
