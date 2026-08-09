(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Aubrey Jaffer")
  (version "SLIB-3b5-r7rs-1")
  (library
    (name
      (slib math-integer))
    (path "slib/math-integer.sld")
    (depends
      (scheme base)
      (srfi 60)))
  (manual "slib-math-integer.html")
  (description "Mathematical functions restricted to exact integers"))
