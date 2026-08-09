(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds bounded-balance-tree))
    (path "pfds/bounded-balance-tree.sld")
    (depends
      (scheme base)
      (scheme case-lambda)
      (pfds list-helpers)))
  (manual "pfds/bounded-balance-tree.html")
  (description "Bounded balance tree"))
