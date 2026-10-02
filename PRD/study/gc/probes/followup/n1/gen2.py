import sys
shape=sys.argv[1]; n=int(sys.argv[2])
H="(import (scheme base) (scheme write))\n"
def wrap(open_,mid,close_): return open_*n + mid + close_*n
if shape=='let':        e=wrap("(let ((a 1)) ","a",")")
elif shape=='let-lit':  e=wrap("(let ((a 1)) ","1",")")
elif shape=='lambda':   e="(" + wrap("(lambda (a) ","a",")") + " 1)"   # returns a procedure chain; we call outermost only
elif shape=='thunk':    e=wrap("((lambda () ","1","))")
elif shape=='if':       e=wrap("(if #t ","1"," 0)")
elif shape=='app':      e=wrap("(+ 1 ","0",")")
elif shape=='begin':    e=wrap("(begin ","1",")")
elif shape=='quote':    e="(quote " + wrap("(","",")") + ")"
elif shape=='qq':       e="(quasiquote " + wrap("(","",")") + ")"
elif shape=='qq-unq':   e="(quasiquote " + wrap("(","(unquote (+ 1 2))",")") + ")"
elif shape=='vec':      e="(quote " + wrap("#(","",")") + ")"
elif shape=='and':      e=wrap("(and #t ","1",")")
elif shape=='cond':     e=wrap("(cond (#t ","1","))")
else: raise SystemExit("bad shape")
if shape in ('quote','qq','qq-unq','vec'):
    print(H + "(define x " + e + ")\n(display (if (pair? x) 'pair (if (vector? x) 'vector x)))(newline)")
elif shape=='lambda':
    print(H + "(define x " + e + ")\n(display (procedure? x))(newline)")
else:
    print(H + "(define x " + e + ")\n(display x)(newline)")
