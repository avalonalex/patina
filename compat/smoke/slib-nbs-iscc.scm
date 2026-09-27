(import (scheme base) (scheme char) (prefix (slib nbs-iscc) p:)
        (prefix (slib color) c:) (patina compat smoke))
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(check-equal "representative pinned color coordinates" #xFFB5BA (c:color->xRGB (p:nbs-iscc "vivid pink")))
(check-equal "lookup is case insensitive" #t
  (eq? (p:nbs-iscc "vivid pink") (p:nbs-iscc "VIVID PINK")))
(check-equal "missing names return false" #f (p:nbs-iscc "not-a-palette-color"))
(check-equal "empty names return false" #f (p:nbs-iscc ""))
(check-equal "the pinned dictionary retains its name count" 267 (length (p:nbs-iscc-names)))
(check-equal "enumerated names are lowercase and resolve to colors" #t
  (let loop ((names (p:nbs-iscc-names)))
    (or (null? names)
        (and (string=? (car names) (string-downcase (car names)))
             (c:color? (p:nbs-iscc (car names))) (loop (cdr names))))))
(check-equal "lookup returns the dictionary's declared color space" 'sRGB
  (c:color-space (p:nbs-iscc "black")))
(check-equal "palette colors survive textual serialization" #t
  (let ((color (p:nbs-iscc "black")))
    (equal? (c:color->sRGB color)
            (observe (lambda () (c:color->sRGB (c:string->color (c:color->string color))))))))
(smoke-finish)
