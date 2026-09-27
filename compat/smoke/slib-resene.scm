(import (scheme base) (scheme char) (prefix (slib resene) p:)
        (prefix (slib color) c:) (patina compat smoke))
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(check-equal "representative pinned color coordinates" '(73 81 84) (c:color->sRGB (p:resene "abbey")))
(check-equal "lookup is case insensitive" #t
  (eq? (p:resene "abbey") (p:resene "ABBEY")))
(check-equal "missing names return false" #f (p:resene "not-a-palette-color"))
(check-equal "empty names return false" #f (p:resene ""))
(check-equal "the pinned dictionary retains its name count" 1379 (length (p:resene-names)))
(check-equal "enumerated names are lowercase and resolve to colors" #t
  (let loop ((names (p:resene-names)))
    (or (null? names)
        (and (string=? (car names) (string-downcase (car names)))
             (c:color? (p:resene (car names))) (loop (cdr names))))))
(check-equal "lookup returns the dictionary's declared color space" 'sRGB
  (c:color-space (p:resene "zydeco")))
(check-equal "palette colors survive textual serialization" #t
  (let ((color (p:resene "zydeco")))
    (equal? (c:color->sRGB color)
            (observe (lambda () (c:color->sRGB (c:string->color (c:color->string color))))))))
(smoke-finish)
