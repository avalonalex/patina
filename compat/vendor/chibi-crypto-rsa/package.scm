(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi crypto rsa))
    (path "chibi/crypto/rsa.sld")
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
      (srfi 27)
      (chibi bytevector)
      (chibi math prime)))
  (library
    (name
      (chibi crypto rsa-test))
    (path "chibi/crypto/rsa-test.sld")
    (depends
      (scheme base)
      (chibi crypto rsa)
      (chibi crypto sha2)
      (chibi test))
    (use-for test))
  (manual "chibi/crypto/rsa.html")
  (description "RSA public key cryptography implementation.")
  (test "run-tests.scm"))
