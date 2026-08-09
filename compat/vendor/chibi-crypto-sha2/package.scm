(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi crypto sha2))
    (path "chibi/crypto/sha2.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
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
          (chibi bytevector))))
    (depends
      (scheme base)))
  (library
    (name
      (chibi crypto sha2-test))
    (path "chibi/crypto/sha2-test.sld")
    (depends
      (scheme base)
      (chibi crypto sha2)
      (chibi test))
    (use-for test))
  (manual "chibi/crypto/sha2.html")
  (description "Implementation of the SHA-2 (Secure Hash Algorithm) cryptographic hash.")
  (test "run-tests.scm"))
