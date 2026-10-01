import sys
n = int(sys.argv[1]); form = sys.argv[2] if len(sys.argv) > 2 else "(guard (e (#t 0)) (raise 'x))"
stats = len(sys.argv) > 3 and sys.argv[3] == 'stats'
print("(import (scheme base) (scheme write)" + (" (patina debug))" if stats else ")"))
for _ in range(n):
    print(form)
if stats:
    print("(write (assq 'symbols (gc-stats))) (newline)")
