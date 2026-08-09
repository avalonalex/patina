(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi app))
    (path "chibi/app.sld")
    (depends
      (scheme base)
      (scheme read)
      (scheme write)
      (scheme process-context)
      (srfi 1)
      (chibi config)
      (chibi edit-distance)
      (chibi string)))
  (library
    (name
      (chibi app-test))
    (path "chibi/app-test.sld")
    (depends
      (scheme base)
      (chibi app)
      (chibi config)
      (chibi test))
    (use-for test))
  (manual "chibi/app.html")
  (description "Unified command-line option parsing and config management.")
  (test "run-tests.scm"))
