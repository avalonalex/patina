(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi term ansi))
    (path "chibi/term/ansi.sld")
    (depends
      (scheme base)
      (scheme write)
      (scheme process-context)))
  (library
    (name
      (chibi term ansi-test))
    (path "chibi/term/ansi-test.sld")
    (depends
      (scheme base)
      (scheme write)
      (chibi term ansi))
    (use-for test))
  (manual "chibi/term/ansi.html")
  (description "A library to use ANSI escape codes to format text and background color, font weigh, and underlining.")
  (test "run-tests.scm"))
