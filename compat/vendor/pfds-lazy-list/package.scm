(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds lazy-list))
    (path "pfds/lazy-list.sld")
    (depends
      (scheme base)
      (scheme lazy)))
  (manual "pfds/lazy-list.html")
  (description "Odd lazy lists"))
