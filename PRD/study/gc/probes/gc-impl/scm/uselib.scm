(import (scheme base) (scheme write) (patina debug) (churnlib))
(display (assq 'collections (gc-stats))) (display (assq 'pairs (gc-stats))) (newline)
