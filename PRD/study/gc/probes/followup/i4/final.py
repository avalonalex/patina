import sys
kind, n, who = sys.argv[1], int(sys.argv[2]), sys.argv[3]
lib = "(define-library (tmp steady) (export f) (import (scheme base)) (begin (define (f x) (+ x 1))))"
out = ["(import (scheme base) (scheme write)" + (" (patina debug))" if who == "patina" else ")")]
for i in range(n):
    out.append(lib)
    if kind == "redefine": out.append("(import (tmp steady))")
    if kind == "safepoint": out.append("(car '(0))")
if who == "patina":
    if kind == "redefine":
        out.append("(gc)")
        out.append("(let ((s (gc-stats)))\n  (write (- (cdr (assq 'objects s)) (cdr (assq 'free-objects s)))) (newline))")
    else:
        out.append("(let ((s (gc-stats)))\n  (write (list (assq 'collections s) (assq 'last-swept s))) (newline))")
else:
    out.append("(display 'done) (newline)")
print("\n".join(out))
