(package
  (maintainers "Kevin Wortman <kwortman@gmail.com>")
  (authors "Shiro Kawai" " John Cowan" " Thomas Gilray")
  (version "1.0.2")
  (library
    (name
      (generators))
    (path "generators.sld")
    (depends
      (scheme case-lambda)
      (scheme base)))
  (manual "srfi-121/srfi-121.html")
  (description "SRFI 121: Generators reference implementation"))
