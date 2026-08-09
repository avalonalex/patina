(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi xgboost))
    (path "chibi/xgboost.sld")
    (depends
      (chibi)
      (scheme base)
      (scheme write)
      (chibi string)))
  (library
    (name
      (chibi xgboost-test))
    (path "chibi/xgboost-test.sld")
    (depends
      (scheme base)
      (scheme write)
      (srfi 160 base)
      (chibi xgboost)
      (chibi test))
    (use-for test))
  (manual "chibi/xgboost.html")
  (test "run-tests.scm"))
