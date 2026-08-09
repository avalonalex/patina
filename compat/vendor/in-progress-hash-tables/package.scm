(package
  (maintainers "William D Cinger <will@ccs.neu.edu>")
  (authors "William D Cinger <will@ccs.neu.edu>")
  (version "0.0.3")
  (license mit)
  (library
    (name
      (in-progress hash tables))
    (path "in-progress/hash/tables.sld")
    (cond-expand
      ((library (scheme char))
        (depends
          (scheme char)))
      (else
        (depends)))
    (depends
      (scheme base)
      (r6rs hashtables)
      (srfi 114 comparators)))
  (manual "hash-tablesDoc.html")
  (description "Hash tables (HashTablesCowan).")
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
