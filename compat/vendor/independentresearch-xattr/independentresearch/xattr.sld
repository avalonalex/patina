;; Author: lockywolf
;; Time-stamp: <2021-04-10 22:02:44 lockywolf>


(define-library (independentresearch xattr)
  (export lgetxattr lsetxattr)
  (import (chibi)
          (only (scheme small) utf8->string))
  (include-shared "xattr")
  (include "xattr.scm"))

