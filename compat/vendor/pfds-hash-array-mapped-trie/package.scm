(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Ian Price")
  (version "1.0.0")
  (license BSD)
  (library
    (name
      (pfds hash-array-mapped-trie))
    (path "pfds/hash-array-mapped-trie.sld")
    (depends
      (scheme base)
      (scheme case-lambda)
      (pfds alist)
      (pfds bitwise)
      (pfds list-helpers)
      (pfds vector)
      (srfi 60)))
  (manual "pfds/hash-array-mapped-trie.html")
  (description "Hash array mapped tries"))
