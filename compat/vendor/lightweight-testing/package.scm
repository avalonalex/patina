(package
  (maintainers "Flynn Liu")
  (authors "Flynn Liu <54922775+flynn162@users.noreply.github.com>")
  (version "0.1")
  (library
    (name
      (lightweight-testing))
    (path "lightweight-testing.sld")
    (depends
      (scheme base)
      (scheme write)
      (chibi test)))
  (description "SRFI-78 implemented as a wrapper around (chibi test)"))
