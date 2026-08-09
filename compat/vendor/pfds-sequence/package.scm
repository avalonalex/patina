(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds sequence))
    (path "pfds/sequence.sld")
    (depends
      (scheme base)
      (pfds fingertree)
      (pfds list-helpers)))
  (manual "pfds/sequence.html")
  (description "Purely functional sequences"))
