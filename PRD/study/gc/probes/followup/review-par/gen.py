import sys
n=int(sys.argv[1])
print("(import (scheme base) (scheme write))\n(define x " + "(let ((a 1)) "*n + "a" + ")"*n + ")\n(display x)(newline)")
