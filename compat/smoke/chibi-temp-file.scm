(import (scheme base) (scheme file) (scheme write) (chibi temp-file)
        (only (chibi filesystem) create-directory file-directory? delete-file-hierarchy)
        (patina compat smoke))
;; Directory creation/cleanup is portable. call-with-temp-file's raw POSIX
;; descriptor path remains unavailable in Patina; this driver does not claim it.
;; Both Patina backends may run concurrently, and the portable upstream
;; process-id fallback is zero. Give their fixtures separate namespaces.
(cond-expand
  (patina-tree-walker (define template "patina-smoke-temp-tw"))
  (patina (define template "patina-smoke-temp-vm"))
  (else (define template "patina-smoke-temp-reference")))
(define path #f)
(check-equal "directory callback result is returned" 42
  (call-with-temp-dir template (lambda (p preserve) (set! path p) 42)))
(check-equal "normal return removes temporary directory" #f (file-exists? path))
(check-equal "temporary directory exists during callback" #t
  (call-with-temp-dir template (lambda (p preserve) (file-directory? p))))
(check-equal "recursive cleanup removes child files and directories" #t
  (begin
    (call-with-temp-dir template
      (lambda (p preserve)
        (set! path p)
        (create-directory (string-append p "/sub"))
        (call-with-output-file (string-append p "/sub/data") (lambda (out) (display "data" out)))))
    (not (file-exists? path))))
(check-equal "nested calls choose distinct live paths" #t
  (call-with-temp-dir template
    (lambda (outer preserve)
      (call-with-temp-dir template
        (lambda (inner preserve) (and (not (equal? outer inner)) (file-directory? outer) (file-directory? inner)))))))
(call-with-temp-dir template (lambda (p preserve) (set! path p) (preserve)))
(dynamic-wind
  (lambda () #f)
  (lambda () (check-equal "preserve retains the generated directory" #t (file-directory? path)))
  (lambda () (delete-file-hierarchy path)))
(check-equal "preserved fixture is explicitly cleaned" #f (file-exists? path))
(check-equal "optional mode is accepted for directory creation" #t
  (call-with-temp-dir template (lambda (p preserve) (file-directory? p)) #o700))
(smoke-finish)
