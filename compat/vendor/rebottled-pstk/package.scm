(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Wolf-Dieter Busch and Nils Holm and Kenneth Dickey")
  (version "1.7.0")
  (library
    (name
      (rebottled pstk))
    (path "rebottled/pstk.sld")
    (cond-expand
      (chibi
        (depends
          (chibi process)
          (chibi filesystem)
          (chibi match)))
      (gauche
        (depends
          (gauche process)))
      (sagittarius
        (depends
          (rnrs io ports)
          (sagittarius process)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme cxr)
      (scheme read)
      (scheme write)))
  (manual "rebottled-pstk.html")
  (description "Portable Scheme Interface to the Tk GUI Toolkit"))
