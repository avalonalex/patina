(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi edit-distance))
    (path "chibi/edit-distance.sld")
    (depends
      (scheme base)
      (srfi 130)))
  (library
    (name
      (chibi edit-distance-test))
    (path "chibi/edit-distance-test.sld")
    (depends
      (scheme base)
      (chibi edit-distance)
      (chibi test))
    (use-for test))
  (manual "chibi/edit-distance.html")
  (test "run-tests.scm"))
