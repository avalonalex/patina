(package
  (authors "Edwin Watkeys <edwin@edwinwatkeys.com>")
  (version "0.2.0")
  (license mit)
  (library
    (name
      (edn))
    (path "edn.sld")
    (depends
      (scheme base)
      (scheme char)
      (chibi parse)))
  (library
    (name
      (edn-test))
    (path "edn-test.sld")
    (depends
      (scheme base)
      (edn)
      (chibi test))
    (use-for test))
  (manual "edn.html")
  (description "EDN is a data format from the Clojure ecosystem.")
  (test "run-tests.scm"))
