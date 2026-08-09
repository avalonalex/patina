(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi scribble))
    (path "chibi/scribble.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme read)))
  (library
    (name
      (chibi scribble-test))
    (path "chibi/scribble-test.sld")
    (depends
      (scheme base)
      (scheme write)
      (chibi scribble)
      (chibi string)
      (chibi test))
    (use-for test))
  (manual "chibi/scribble.html")
  (description "A library used for parsing \"scribble\" format, introduced by Racket and the format used to write this manual.")
  (test "run-tests.scm"))
