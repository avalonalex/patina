;; SRFI 116: immutable (pure) lists.
;;
;; `116/ilists-base.scm` and `116/ilists-impl.scm` are the SRFI's own
;; reference implementation, byte-identical (John Cowan, MIT); see
;; `PROVENANCE.md`. Only this library declaration is local: upstream's names
;; the library `(ilists ilists)`, which is not what R7RS code imports, and
;; carries a Gauche-only `cond-expand` branch that has no `else`.
;;
;; `(scheme write)` is imported because `ilists-base.scm`'s `write-ipair`
;; calls `write`; upstream's declaration omits it and relies on the host's
;; `(scheme base)` providing it, which Patina's happens to do.
;;
;; The two includes are order-dependent: both files define `imap`, which
;; R7RS 5.3.1 forbids in one body, and the impl's definition is the live one
;; only because it is included second. Recorded rather than edited — both
;; files are byte-identical upstream apart from the marked edits.
(define-library (srfi 116)
  (import (scheme base) (scheme write) (srfi 128))
  (export
    iq ipair ilist xipair ipair* make-ilist ilist-copy ilist-tabulate
    iiota ipair? proper-ilist? ilist? dotted-ilist? not-ipair? null-ilist?
    ilist= icar icdr ilist-ref ifirst isecond ithird ifourth ififth isixth
    iseventh ieighth ininth itenth icaar icadr icdar icddr icaaar icaadr
    icadar icaddr icdaar icdadr icddar icdddr icaaaar icaaadr icaadar
    icaaddr icadaar icadadr icaddar icadddr icdaaar icdaadr icdadar
    icdaddr icddaar icddadr icdddar icddddr icar+icdr itake idrop
    ilist-tail itake-right idrop-right isplit-at ilast last-ipair ilength
    iappend iconcatenate ireverse iappend-reverse izip iunzip1 iunzip2
    iunzip3 iunzip4 iunzip5 icount imap ifor-each ifold iunfold ipair-fold
    ireduce ifold-right iunfold-right ipair-fold-right ireduce-right
    iappend-map ipair-for-each ifilter-map imap-in-order ifilter
    ipartition iremove imember imemq imemv ifind ifind-tail iany ievery
    ilist-index itake-while idrop-while ispan ibreak idelete
    idelete-duplicates iassoc iassq iassv ialist-cons ialist-delete
    replace-icar replace-icdr pair->ipair ipair->pair list->ilist
    ilist->list tree->itree itree->tree gtree->itree gtree->tree iapply
    ipair-comparator ilist-comparator make-ilist-comparator
    make-improper-ilist-comparator make-ipair-comparator
    make-icar-comparator make-icdr-comparator)
  (include "116/ilists-base.scm")
  (include "116/ilists-impl.scm"))
