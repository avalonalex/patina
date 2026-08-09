(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds fingertree))
    (path "pfds/fingertree.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (pfds list-helpers)))
  (manual "pfds/fingertree.html")
  (description "Fingertree: A simple general-purpose data structure"))
