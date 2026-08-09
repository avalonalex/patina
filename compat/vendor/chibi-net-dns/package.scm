(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.2")
  (license bsd)
  (library
    (name
      (chibi net dns))
    (path "chibi/net/dns.sld")
    (depends
      (scheme base)
      (scheme file)
      (scheme write)
      (srfi 26)
      (srfi 27)
      (srfi 33)
      (srfi 95)
      (srfi 130)
      (chibi optional)
      (chibi net)))
  (library
    (name
      (chibi net dns-test))
    (path "chibi/net/dns-test.sld")
    (depends
      (scheme base)
      (chibi test)
      (chibi net dns))
    (use-for test))
  (manual "chibi/net/dns.html")
  (description "Domain Name Service library, with high-level utilities for address, mx and text record lookups.")
  (test "run-tests.scm"))
