(import (scheme base) (scheme char) (prefix (slib saturate) p:)
        (prefix (slib color) c:) (patina compat smoke))
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(check-equal "representative pinned color coordinates" "CIEXYZ:0.735484/0.264516/0" (c:color->string (p:saturate "red")))
(check-equal "lookup is case insensitive" #t
  (eq? (p:saturate "red") (p:saturate "RED")))
(check-equal "missing names return false" #f (p:saturate "not-a-palette-color"))
(check-equal "empty names return false" #f (p:saturate ""))
(check-equal "the pinned dictionary retains its name count" 19 (length (p:saturate-names)))
(check-equal "enumerated names are lowercase and resolve to colors" #t
  (let loop ((names (p:saturate-names)))
    (or (null? names)
        (and (string=? (car names) (string-downcase (car names)))
             (c:color? (p:saturate (car names))) (loop (cdr names))))))
(check-equal "lookup returns the dictionary's declared color space" 'CIEXYZ
  (c:color-space (p:saturate "purplish red")))
(check-equal "palette colors survive textual serialization" #t
  (let ((color (p:saturate "purplish red")))
    (equal? (c:color->sRGB color)
            (observe (lambda () (c:color->sRGB (c:string->color (c:color->string color))))))))
(smoke-finish)
