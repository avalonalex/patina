(import (scheme base) (nieper rbtree) (patina debug) (scheme write))
(write (assq (quote collections) (gc-stats)))
(newline)
