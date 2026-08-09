;; -*- mode:scheme; -*-
;; Time-stamp: <2021-04-10 22:02:25 lockywolf>

;;> set an eXtended ATTRibute called \var{name}
;;> from fs object \var{path}
;;> Does _not_ follow symlinks.
(define (lgetxattr path name)
  (let ((s (%lgetxattr-size path name)))
    (if (= -1 s)
       #f
       (let* ((buf (make-bytevector s 1))
              (buf2 (utf8->string buf))
              (nread-bytes (%lgetxattr-value! path name buf2 s))
              )
         (if (not (= nread-bytes s))
            #f
            buf2)))))

;;> set an eXtended ATTRibute with \var{name}
;;> to value \var{value} to a fs object at
;;> \var{path}
;;> Does _not_ follow symlinks.
(define (lsetxattr path name value)
  (if (= -1 (%lsetxattr path name value (bytevector-length value) 0))
     #f
     #t))
