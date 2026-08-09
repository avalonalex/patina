(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Mikael Djurfeldt")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib topological-sort))
    (path "slib/topological-sort.sld")
    (depends
      (scheme base)
      (srfi 69)))
  (manual "slib-topological-sort.html")
  (description "Topological sort"))
