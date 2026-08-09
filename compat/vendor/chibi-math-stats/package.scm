(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi math stats))
    (path "chibi/math/stats.sld")
    (depends
      (scheme base)
      (scheme inexact)
      (scheme write)
      (scheme comparator)
      (scheme flonum)
      (scheme hash-table)
      (scheme list)
      (scheme mapping)
      (scheme set)
      (scheme vector)
      (scheme sort)
      (srfi 27)
      (srfi 95)
      (chibi optional)))
  (library
    (name
      (chibi math stats-test))
    (path "chibi/math/stats-test.sld")
    (depends
      (scheme base)
      (scheme inexact)
      (chibi math stats)
      (chibi test))
    (use-for test))
  (manual "chibi/math/stats.html")
  (description "Statistics is the branch of mathematics dealing with the collection and analysis of data.")
  (test "run-tests.scm"))
