import sys
mode, n = sys.argv[1], int(sys.argv[2])
out = ["(import (scheme base) (scheme write) (patina debug))"]
for i in range(n):
    if mode == "only":
        out.append("(define-library (tmp steady) (export f) (import (only (scheme base) +)) (begin (define (f x) (+ x 1))))")
        out.append("(import (tmp steady))")
    elif mode == "barezero":
        out.append("(define-library (tmp steady) (export f) (import (scheme base)) (begin (define (f x) (+ x 1))))")
        out.append("(car '(0))")
    elif mode == "plain":
        out.append("(define (f x) (+ x 1))")
        out.append("(car '(0))")
out.append("(gc)")
out.append("(write (gc-stats)) (newline)")
print("\n".join(out))
