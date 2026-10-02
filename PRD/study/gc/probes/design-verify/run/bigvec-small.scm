(import (scheme base) (scheme write) (patina debug))
(write (assq 'collections (gc-stats))) (newline)
