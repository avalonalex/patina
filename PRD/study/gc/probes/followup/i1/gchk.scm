(import (scheme base) (scheme write) (patina debug) (t g))
(write (list (gx (e (#t 0)) (raise 'x)) (gy (e ((symbol? e) e)) (raise 'y)) (gx (e (#f 0)) 5)))
(newline)
