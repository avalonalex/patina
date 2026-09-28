;; #530: the pinned boundary data omitted every Unicode 6.3 singleton,
;; and the grapheme compiler used V where its Hangul alternative needs LV.
;; Stable old codepoints keep this independent of newer Unicode versions.
(import (scheme base) (srfi 115) (srfi 64))
(test-begin "regex-graphemes")
(define (cluster . codepoints) (list->string (map integer->char codepoints)))
(test-assert "grapheme includes an LV syllable and trailing jamo"
  (regexp-matches? 'grapheme (cluster #xAC00 #x11A8)))
(test-assert "grapheme includes a spacing-mark singleton"
  (regexp-matches? 'grapheme (cluster #x61 #x0903)))
(test-assert "grapheme includes an Extend singleton"
  (regexp-matches? 'grapheme (cluster #x05D0 #x05BF)))
(test-assert "grapheme includes an LVT syllable and trailing jamo"
  (regexp-matches? 'grapheme (cluster #xAC01 #x11A8)))
(test-assert "distinct Hangul syllables are distinct graphemes"
  (not (regexp-matches? 'grapheme (cluster #xAC00 #xAC1C))))
(test-assert "every LV syllable can combine with trailing jamo"
  (let loop ((cp #xAC00))
    (or (> cp #xD7A3)
        (and (regexp-matches? 'grapheme (cluster cp #x11A8))
             (loop (+ cp 28))))))
(test-end)
