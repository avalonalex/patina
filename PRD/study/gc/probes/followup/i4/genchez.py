import sys
mode, n = sys.argv[1], int(sys.argv[2])
out = []
for i in range(n):
    out.append("(library (tmp steady) (export f) (import (rnrs)) (define (f x) (+ x 1)))")
    if mode == "import":
        out.append("(import (tmp steady))")
out.append("(display 'done) (newline)")
print("\n".join(out))
