import sys
mode, n = sys.argv[1], int(sys.argv[2])
out = ["(import (scheme base) (scheme write) (patina debug))"]
for i in range(n):
    out.append("(define-library (tmp steady) (export f) (import (scheme base)) (begin (define (f x) (+ x 1))))")
    if mode in ("import", "call"):
        out.append("(import (tmp steady))")
    if mode == "call":
        out.append("(f 1)")
out.append("(gc)")
out.append("(write (gc-stats)) (newline)")
print("\n".join(out))
