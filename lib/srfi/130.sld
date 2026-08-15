;; SRFI 130: Cursor-based string library
;;
;; Alex Shinn's implementation, from chibi-scheme's own tree
;; (lib/srfi/130.sld and lib/srfi/130.scm) pinned at commit
;; f26603620cc479b4e404db5790a35f06946368bf. BSD 3-Clause; the full text is in
;; lib/chibi/PROVENANCE.md § Licence (same author, same licence), rather than
;; only behind a link. There is no snowball release of this
;; one, which is why it is recorded here rather than in PROVENANCE.md with
;; the tarball-pinned trees.
;;
;; The library form below is unchanged from upstream — this header is the only
;; addition to the file. `130.scm` carries exactly one deviation, marked
;; `;; PATINA LOCAL EDIT:` at its site: upstream's
;; `string-drop` calls `(substring str n)`, and R7RS 6.7 gives `substring`
;; exactly three arguments. The two-argument form is a chibi extension that
;; Gauche and Chez reject as we do, so this is upstream depending on its own
;; reader rather than a gap in Patina — see PRD/TRACK_L_SNOW_LIBRARIES_PRD.md
;; section 6 for the same verdict applied to other chibi-only constructs.
;;
;; It is written against `(chibi string)`, bundled here; lib/chibi/PROVENANCE.md
;; is where that tree's record lives, including why its cursors are integers.

(define-library (srfi 130)
  (export
   ;; Cursor operations
   string-cursor?
   string-cursor-start    string-cursor-end
   string-cursor-next     string-cursor-prev
   string-cursor-forward  string-cursor-back
   string-cursor=?
   string-cursor<?        string-cursor>?
   string-cursor<=?       string-cursor>=?
   string-cursor-diff
   string-cursor->index   string-index->cursor
   ;; Predicates
   string-null? string-every string-any
   ;; Constructors
   string-tabulate string-unfold string-unfold-right
   ;; Conversion
   string->list/cursors string->vector/cursors
   reverse-list->string string-join
   ;; Selection
   string-ref/cursor
   substring/cursors  string-copy/cursors
   string-take        string-take-right
   string-drop        string-drop-right
   string-pad         string-pad-right 
   string-trim        string-trim-right string-trim-both
   ;; Prefixes & suffixes
   string-prefix-length    string-suffix-length
   string-prefix?          string-suffix?    
   ;; Searching
   string-index string-index-right
   string-skip  string-skip-right
   string-contains string-contains-right
   ;; The whole string
   string-reverse
   string-concatenate  string-concatenate-reverse
   string-fold         string-fold-right
   string-for-each-cursor
   string-replicate    string-count
   string-replace      string-split
   string-filter       string-remove)
  (import (scheme base)
          (scheme char) (scheme write)
          (rename (chibi string)
                  (string-index->cursor %string-index->cursor)
                  (string-cursor->index %string-cursor->index)
                  (string-cursor-next %string-cursor-next)
                  (string-cursor-prev %string-cursor-prev)
                  (string-fold %string-fold)
                  (string-fold-right %string-fold-right)
                  (string-contains %string-contains)
                  (string-join %string-join)
                  (string-prefix? %string-prefix?)
                  (string-suffix? %string-suffix?)))
  (begin
    (define (string-cursor-next str cursor)
      (if (string-cursor? cursor)
          (%string-cursor-next str cursor)
          (+ cursor 1)))
    (define (string-cursor-prev str cursor)
      (if (string-cursor? cursor)
          (%string-cursor-prev str cursor)
          (- cursor 1)))
    (define (string-index->cursor str i)
      (if (string-cursor? i)
          i
          (%string-index->cursor str i)))
    (define (string-cursor->index str cursor)
      (if (string-cursor? cursor)
          (%string-cursor->index str cursor)
          cursor)))
  (include "130.scm"))
