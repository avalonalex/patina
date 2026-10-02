(import (scheme base) (scheme write) (patina debug))
(guard (e (#t 0)) (raise 'x))
(guard (e (#t 0)) (raise 'x))
(write (assq 'symbols (gc-stats))) (newline)
