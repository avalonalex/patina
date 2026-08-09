(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Radey Shouman")
  (version "SLIB-3b5-r7rs")
  (library
    (name
      (slib filename))
    (path "slib/filename.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme file)
      (slib common)))
  (manual "slib-filename.html")
  (description "String matching for filenames (glob, a la BASH)"))
