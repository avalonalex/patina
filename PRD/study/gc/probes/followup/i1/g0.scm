(import (scheme base) (scheme write) (patina debug))
(write (assq 'symbols (gc-stats))) (newline)
