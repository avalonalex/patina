(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.7.3.1")
  (license bsd)
  (library
    (name
      (chibi show))
    (path "chibi/show.sld")
    (depends
      (scheme base)
      (scheme char)
      (chibi show base)
      (scheme write)))
  (library
    (name
      (chibi show base))
    (path "chibi/show/base.sld")
    (depends
      (scheme base)
      (scheme write)
      (scheme complex)
      (scheme inexact)
      (srfi 1)
      (srfi 69)
      (chibi string)
      (chibi monad environment)))
  (library
    (name
      (chibi show pretty))
    (path "chibi/show/pretty.sld")
    (depends
      (scheme base)
      (scheme write)
      (chibi show)
      (chibi show base)
      (srfi 1)
      (srfi 69)
      (chibi string)))
  (library
    (name
      (chibi show-test))
    (path "chibi/show-test.sld")
    (depends
      (scheme base)
      (scheme read)
      (chibi test)
      (chibi show)
      (chibi show base)
      (chibi show pretty))
    (use-for test))
  (manual "chibi/show.html" "chibi/show/pretty.html")
  (description "A library of procedures for formatting Scheme objects to text in various ways, and for easily concatenating, composing and extending these formatters.")
  (test "run-tests.scm"))
