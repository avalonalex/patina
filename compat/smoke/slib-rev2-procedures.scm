(import (scheme base) (prefix (slib rev2-procedures) r:) (patina compat smoke))
(check-equal "leftward overlapping move preserves the source sequence" "cdefef"
  (let ((s (string-copy "abcdef"))) (r:substring-move-left! s 2 6 s 0) s))
(check-equal "rightward overlapping move preserves the source sequence" "ababcd"
  (let ((s (string-copy "abcdef"))) (r:substring-move-right! s 0 4 s 2) s))
(check-equal "left move honors independent source and destination bounds" '("abcdef" "-bcd--")
  (let ((a (string-copy "abcdef")) (b (string-copy "------")))
    (r:substring-move-left! a 1 4 b 1) (list a b)))
(check-equal "right move honors independent source and destination bounds" "--bcd-"
  (let ((b (string-copy "------"))) (r:substring-move-right! "abcdef" 1 4 b 2) b))
(check-equal "empty moves leave their destinations untouched" "abc"
  (let ((s (string-copy "abc")))
    (r:substring-move-left! s 3 3 s 3)
    (r:substring-move-right! s 3 3 s 3) s))
(check-equal "fill only changes the requested half-open interval" "aXXXef"
  (let ((s (string-copy "abcdef"))) (r:substring-fill! s 1 4 #\X) s))
(check-equal "an empty fill at the end is a no-op" "abc"
  (let ((s (string-copy "abc"))) (r:substring-fill! s 3 3 #\X) s))
(check-equal "string-null distinguishes empty and nonempty strings" '(#t #f)
  (list (r:string-null? "") (r:string-null? "x")))
(check-equal "comparison aliases retain chained numeric semantics" '(#t #t #t #t #t)
  (list (r:<? -1 0 1) (r:<=? 1 1 2) (r:=? 2 2.0 2)
        (r:>? 3 2 1) (r:>=? 2 2 1)))
(check-equal "comparison aliases reject incorrectly ordered sequences" '(#f #f #f #f #f)
  (list (r:<? 1 1) (r:<=? 2 1) (r:=? 1 2) (r:>? 1 1) (r:>=? 1 2)))
(check-error "move cannot write outside the destination" (r:substring-move-left! "ab" 0 2 (string #\x) 0))
(check-error "fill cannot write outside the destination" (r:substring-fill! (string #\x) 0 2 #\y))
(smoke-finish)
