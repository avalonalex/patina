(package
  (maintainers "Lassi Kortela <lassi@lassi.io>")
  (authors "Lassi Kortela")
  (version "0.2")
  (license ISC)
  (library
    (name
      (lassik dockerfile))
    (path "lassik/dockerfile.sld")
    (depends
      (scheme base)
      (scheme write)
      (srfi 1)
      (chibi match)
      (lassik unpack-assoc)
      (lassik shell-quote)))
  (description "Scheme DSL to build Dockerfiles")
  (test "lassik/dockerfile-test.scm")
  (test-depends
    (scheme base)
    (scheme write)
    (lassik dockerfile)))
