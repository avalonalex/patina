(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.1")
  (license bsd)
  (library
    (name
      (chibi mecab))
    (path "chibi/mecab.sld")
    (depends
      (chibi)
      (scheme base)
      (srfi 130)
      (chibi assert)
      (chibi optional)))
  (library
    (name
      (chibi mecab-test))
    (path "chibi/mecab-test.sld")
    (depends
      (scheme base)
      (chibi mecab)
      (chibi test))
    (use-for test))
  (manual "chibi/mecab.html")
  (description "A wrapper around MeCab, a part-of-speech and morphological analyzer for Japanese.")
  (test "run-tests.scm"))
