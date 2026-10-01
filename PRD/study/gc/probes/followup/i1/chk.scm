(import (scheme base) (scheme write) (t m))
(write (list (m0 1) (m1 2) (m2 3) (m3 4) (guard (e (#t 0)) (raise 'x))))
(newline)
