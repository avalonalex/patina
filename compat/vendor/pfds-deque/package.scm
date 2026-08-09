(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds deque))
    (path "pfds/deque.sld")
    (depends
      (scheme base)
      (pfds lazy-list)
      (pfds list-helpers)))
  (manual "pfds/deque.html")
  (description "Purely functional deques"))
