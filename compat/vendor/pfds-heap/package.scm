(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds heap))
    (path "pfds/heap.sld")
    (depends
      (scheme base)
      (pfds list-helpers)))
  (manual "pfds/heap.html")
  (description "Heap data structure"))
