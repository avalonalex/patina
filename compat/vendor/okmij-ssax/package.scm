(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Oleg Kiselyov <oleg@okmij.org>")
  (version "5.4")
  (license public-domain)
  (library
    (name
      (okmij ssax))
    (path "okmij/ssax.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme cxr)
      (scheme write)
      (srfi 1)))
  (library
    (name
      (okmij ssax-test))
    (path "okmij/ssax-test.sld")
    (depends
      (scheme base)
      (scheme write)
      (srfi 1)
      (srfi 130)
      (chibi test)
      (okmij ssax))
    (use-for test))
  (manual "SXML.html")
  (description "Functional XML parsing framework")
  (test "run-tests.scm"))
