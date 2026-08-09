(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds queue))
    (path "pfds/queue.sld")
    (depends
      (scheme base)
      (pfds list-helpers)
      (pfds lazy-list)))
  (manual "pfds/queue.html")
  (description "Purely functional queues"))
