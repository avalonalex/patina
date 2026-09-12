;; Install examples/snow-pfds.lock.json with scripts/install_snow_locked.py,
;; then pass that project's library root explicitly with -A.
(import (scheme base) (scheme write) (pfds queue))
(write (queue->list (enqueue (list->queue '(1 2)) 3)))
(newline)
