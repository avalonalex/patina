(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi ssl))
    (path "chibi/ssl.sld")
    (depends
      (chibi)
      (scheme base)
      (srfi 33)
      (chibi io)))
  (library
    (name
      (chibi ssl-test))
    (path "chibi/ssl-test.sld")
    (depends
      (scheme base)
      (chibi ssl)
      (chibi test))
    (use-for test))
  (manual "chibi/ssl.html")
  (description "Basic bindings for establishing SSL connections.")
  (test "run-tests.scm"))
