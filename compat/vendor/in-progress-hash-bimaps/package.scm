(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.3")
  (license mit)
  (library
    (name
      (in-progress hash bimaps))
    (path "in-progress/hash/bimaps.sld")
    (depends
      (scheme base)
      (r6rs hashtables)
      (in-progress hash tables)))
  (manual "bimapsDoc.html")
  (description "Bimaps (HashTablesCowan).")
  (test "in-progress/hash/tables-test.sps")
  (test-depends
    (scheme base)
    (scheme char)
    (scheme write)
    (scheme process-context)
    (srfi 114 comparators)
    (r6rs sorting)
    (in-progress hash tables)
    (in-progress hash bimaps)))
