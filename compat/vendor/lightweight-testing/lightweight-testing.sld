;; -*- scheme -*-
;; SPDX-License-Identifier: Apache-2.0
;; Copyright 2020 Flynn Liu

(define-library (lightweight-testing)
  (import (scheme base) (scheme write)
          (chibi test)
          )
  (export check check-report check-set-mode! check-reset! check-passed?)
  (include "./lightweight-testing.scm"))
