(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.9.0")
  (license bsd)
  (library
    (name
      (chibi iset))
    (path "chibi/iset.sld")
    (depends
      (scheme base)
      (chibi iset base)
      (chibi iset iterators)
      (chibi iset constructors)))
  (library
    (name
      (chibi iset base))
    (path "chibi/iset/base.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (srfi 9)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (cond-expand
      (chicken
        (depends))
      (else
        (depends)))
    (depends))
  (library
    (name
      (chibi iset iterators))
    (path "chibi/iset/iterators.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (srfi 9)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (chibi iset base)))
  (library
    (name
      (chibi iset constructors))
    (path "chibi/iset/constructors.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (chibi iset base)
      (chibi iset iterators)))
  (library
    (name
      (chibi iset optimize))
    (path "chibi/iset/optimize.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)
          (srfi 9)))
      (else
        (depends
          (scheme base))))
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (chibi iset base)
      (chibi iset iterators)
      (chibi iset constructors)))
  (library
    (name
      (chibi iset-test))
    (path "chibi/iset-test.sld")
    (depends
      (scheme base)
      (scheme write)
      (srfi 1)
      (chibi iset)
      (chibi iset optimize)
      (chibi test))
    (use-for test))
  (manual "chibi/iset.html" "chibi/iset/optimize.html")
  (description "A space efficient integer set (iset) implementation, optimized for minimal space usage and fast membership lookup.")
  (test "run-tests.scm"))
