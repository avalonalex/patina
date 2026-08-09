(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Jonathan Kraut <jak76@columbia.edu>")
  (version "0.2.1")
  (license public-domain)
  (library
    (name
      (jkode sassy))
    (path "jkode/sassy.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)
      (scheme eval)
      (scheme file)
      (scheme read)
      (scheme repl)
      (scheme write)
      (srfi 1)
      (srfi 142)
      (srfi 69)
      (srfi 98)))
  (library
    (name
      (jkode sassy-test))
    (path "jkode/sassy-test.sld")
    (depends
      (scheme base)
      (jkode sassy)
      (chibi test))
    (use-for test))
  (manual "http://sassy.sourceforge.net/sassy.html")
  (description "A portable assembler for x86 processors")
  (test "run-tests.scm"))
