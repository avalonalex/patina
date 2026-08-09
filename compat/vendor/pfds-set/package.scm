(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds set))
    (path "pfds/set.sld")
    (depends
      (scheme base)
      (pfds bounded-balance-tree)
      (pfds list-helpers)))
  (manual "pfds/set.html")
  (description "Purely functional sets"))
