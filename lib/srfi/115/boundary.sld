;;;; SPDX-FileCopyrightText: 2015 Alex Shinn
;;;;
;;;; SPDX-License-Identifier: CC0-1.0

;; Character sets for Unicode boundaries, TR29.
;; This code is written by Alex Shinn and placed in the
;; Public Domain.  All warranties are disclaimed.

;;> Char-sets used for
;;> \hyperlink["http://unicode.org/reports/tr29/"]{TR29} word
;;> boundaries.

(define-library (srfi 115 boundary)
  (cond-expand
   (chibi (import (chibi)))
   (else (import (scheme base))))
  ;; PATINA DEVIATION (#431): upstream tests `(library (chibi char-set))` here.
  ;; `115.sld`, the only client of these sets, chooses by the `chibi` *feature*
  ;; and off chibi takes `(srfi 14)`; choosing by *availability* here meant
  ;; that with any `(chibi char-set)` on the search path the sets below were
  ;; one library's records and the regexp compiler's `char-set?` another's, and
  ;; `(srfi 115)` failed to load. Asking the question its client asks changes
  ;; nothing on chibi, where the feature is set and the library exists.
  (cond-expand
   (chibi (import (chibi char-set)))
   (else
    (import (srfi 14))
    (begin (define (immutable-char-set cs) cs))))
  (export char-set:regional-indicator
          char-set:extend-or-spacing-mark
          char-set:hangul-l
          char-set:hangul-v
          char-set:hangul-t
          char-set:hangul-lv
          char-set:hangul-lvt)
  ;; generated with:
  ;; tools/extract-unicode-props.scm --derived GraphemeBreakProperty.txt
  ;;   Control extend-or-spacing-mark=Extend,SpacingMark Regional_Indicator
  ;;   hangul-l=:L hangul-v=:V hangul-t=:T hangul-lv=:LV hangul-lvt=:LVT
  (include "boundary.scm"))
