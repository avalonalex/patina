(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds bitwise))
    (path "pfds/bitwise.sld")
    (depends
      (scheme base)
      (srfi 60)))
  (manual "pfds/bitwise.html")
  (description "Bitwise arithmetic utilities"))
