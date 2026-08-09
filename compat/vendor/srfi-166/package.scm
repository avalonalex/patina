(package
  (maintainers "Alex Shinn <alexshinn@gmail.com>")
  (authors "Alex Shinn <alexshinn@gmail.com>")
  (version "0.8.0")
  (license bsd)
  (library
    (name
      (srfi 166))
    (path "srfi/166.sld")
    (depends
      (srfi 166 base)
      (srfi 166 pretty)
      (srfi 166 columnar)
      (srfi 166 unicode)
      (srfi 166 color)))
  (library
    (name
      (srfi 166 base))
    (path "srfi/166/base.sld")
    (cond-expand
      (chibi
        (depends
          (chibi)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (scheme complex)
      (scheme inexact)
      (scheme repl)
      (scheme write)
      (srfi 1)
      (srfi 69)
      (srfi 130)
      (srfi 165)
      (chibi show shared)))
  (library
    (name
      (srfi 166 pretty))
    (path "srfi/166/pretty.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme write)
      (chibi show shared)
      (srfi 1)
      (srfi 69)
      (srfi 130)
      (srfi 166 base)
      (srfi 166 color)))
  (library
    (name
      (srfi 166 color))
    (path "srfi/166/color.sld")
    (depends
      (scheme base)
      (srfi 130)
      (srfi 165)
      (srfi 166 base)))
  (library
    (name
      (srfi 166 columnar))
    (path "srfi/166/columnar.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme file)
      (srfi 1)
      (srfi 117)
      (srfi 130)
      (srfi 166 base)
      (chibi optional)))
  (library
    (name
      (srfi 166 unicode))
    (path "srfi/166/unicode.sld")
    (depends
      (scheme base)
      (scheme char)
      (srfi 130)
      (srfi 151)
      (srfi 166 base)))
  (library
    (name
      (chibi show shared))
    (path "chibi/show/shared.sld")
    (depends
      (scheme base)
      (scheme write)
      (srfi 69)))
  (manual "https://srfi.schemers.org/srfi-166/srfi-166.html"))
