(define-c-library libc
                  '("stdlib.h"
                    "stdio.h"
                    "string.h"
                    "dirent.h"
                    "sys/stat.h"
                    "sys/statvfs.h"
                    "sys/types.h"
                    "unistd.h"
                    "pwd.h"
                    "grp.h"
                    "fcntl.h")
                  #f
                  '())

(define-c-procedure c-perror libc 'perror 'void '(pointer))
(define-c-procedure c-mkdir libc 'mkdir 'int '(pointer int))
(define-c-procedure c-mkfifo libc 'mkfifo 'int '(pointer int))
(define-c-procedure c-readlink libc 'readlink 'int '(pointer pointer int))
(define-c-procedure c-rmdir libc 'rmdir 'int '(pointer))
(define-c-procedure c-stat libc 'stat 'int '(pointer pointer))
(define-c-procedure c-lstat libc 'stat 'int '(pointer pointer))
(define-c-procedure c-open libc 'open 'int '(pointer int))
(define-c-procedure c-opendir libc 'opendir 'pointer '(pointer))
(define-c-procedure c-dirfd libc 'dirfd 'int '(pointer))
(define-c-procedure c-readdir libc 'readdir 'pointer '(pointer))
(define-c-procedure c-close libc 'close 'int '(int))
(define-c-procedure c-closedir libc 'closedir 'int '(pointer))
(define-c-procedure c-realpath libc 'realpath 'pointer '(pointer pointer))
(define-c-procedure c-chmod libc 'chmod 'int '(pointer int))
(define-c-procedure c-getpid libc 'getpid 'int '())
(define-c-procedure c-time libc 'time 'int '(pointer))
(define-c-procedure c-srand libc 'srand 'void '(int))
(define-c-procedure c-rand libc 'rand 'int '())
(define-c-procedure c-getcwd libc 'getcwd 'pointer '(pointer int))
(define-c-procedure c-chdir libc 'chdir 'int '(pointer))
(define-c-procedure c-getuid libc 'getuid 'int '())
(define-c-procedure c-getgid libc 'getgid 'int '())
(define-c-procedure c-geteuid libc 'geteuid 'int '())
(define-c-procedure c-getegid libc 'getegid 'int '())
(define-c-procedure c-getgroups libc 'getgroups 'int '(int pointer))
(define-c-procedure c-getpwuid libc 'getpwuid 'pointer '(int))
(define-c-procedure c-getpwnam libc 'getpwnam 'pointer '(pointer))
(define-c-procedure c-getgrgid libc 'getgrgid 'pointer '(int))
(define-c-procedure c-getgrnam libc 'getgrnam 'pointer '(pointer))
(define-c-procedure c-setenv libc 'setenv 'int '(pointer pointer int))
(define-c-procedure c-unsetenv libc 'unsetenv 'int '(pointer))
(define-c-procedure c-rename libc 'rename 'int '(pointer pointer))
(define-c-procedure c-link libc 'link 'int '(pointer pointer))
(define-c-procedure c-slink libc 'link 'int '(pointer pointer))
(define-c-procedure c-chown libc 'chown 'int '(pointer int int))
(define-c-procedure c-clock-gettime libc 'clock_gettime 'int '(int pointer))
(define-c-procedure c-nice libc 'nice 'int '(int))
(define-c-procedure c-umask libc 'umask 'uint '(int))
(define-c-procedure
  c-utimensat libc 'utimensat 'int '(int pointer pointer int))
(define-c-procedure c-truncate libc 'truncate 'int '(pointer int))
(define-c-procedure c-statvfs libc 'statvfs 'int '(pointer pointer))

(define slash (cond-expand (windows "\\") (else "/")))
(define randomized? #f)

(define (string-split str mark)
  (let* ((str-l (string->list str))
         (res (list))
         (last-index 0)
         (index 0)
         (splitter
           (lambda (c)
             (cond ((char=? c mark)
                    (begin
                      (set! res
                        (append res
                                (list (string-copy str last-index index))))
                      (set! last-index (+ index 1))))
                   ((equal? (length str-l) (+ index 1))
                    (set! res
                      (append res
                              (list (string-copy str
                                                 last-index
                                                 (+ index 1)))))))
             (set! index (+ index 1)))))
    (for-each splitter str-l)
    res))

(define (string-char-replace replace-in replace-this replace-with)
  (let ((result ""))
    (list->string
      (for-each
        (lambda (c)
          (if (char=? c replace-this)
            (set! result (string-append result replace-with))
            (set! result (string-append result (string c)))))
        (string->list replace-in)))
    result))

(define (random-to max)
  (when (not randomized?)
    (c-srand (c-time (c-bytevector-null)))
    (set! randomized? #t))
  (modulo (c-rand) max))

(define (random-string size)
  (letrec
    ((looper
       (lambda (result integer)
         (cond ((= (string-length result) size) result)
               ((or (< integer 0)
                    (> integer 128))
                (looper result (random-to 128)))
               (else
                 (let ((char (integer->char integer)))
                   (if (not (or (char-alphabetic? char)
                                (char-numeric? char)))
                     (looper result (c-rand))
                     (looper (string-append result
                                            (string (integer->char integer)))
                             (random-to 128)))))))))
    (looper "" (random-to 128))))

(define-record-type <file-info>
  (make-file-info device
                  inode
                  mode
                  nlinks
                  uid
                  gid
                  rdev
                  size
                  blksize
                  blocks
                  atime
                  mtime
                  ctime
                  fname/port
                  follow?)
  file-info?
  (device file-info:device)
  (inode file-info:inode)
  (mode file-info:mode)
  (nlinks file-info:nlinks)
  (uid file-info:uid)
  (gid file-info:gid)
  (rdev file-info:rdev)
  (size file-info:size)
  (blksize file-info:blksize)
  (blocks file-info:blocks)
  (atime file-info:atime)
  (mtime file-info:mtime)
  (ctime file-info:ctime)
  (fname/port file-info:fname/port)
  (follow? file-info:follow?))

(define (file-info-directory? file-info)
  (when (not (file-info? file-info))
    (error "file-info-directory? error: file-info must be <file-info> record"
           file-info))
  (let* ((file-info:fname/port*
           (string->c-bytevector (file-info:fname/port file-info)))
         (handle (c-open file-info:fname/port* 2))
         (result
           (cond ((> handle 0) (c-close handle) #f)
                 (else #t))))
    (c-bytevector-free file-info:fname/port*)
    result))

(define-c-struct-type stat-struct
                      `((st_dev int)
                        (st_ino uint)
                        (st_mode uint)
                        (st_nlink int)
                        (st_uid uint)
                        (st_gid uint)
                        (st_rdev int)
                        (st_size int)
                        (st_blksize int)
                        (st_blocks int)
                        (st_atim.tv_sec long)
                        (st_atim.tv_nsec long)
                        (st_mtim.tv_sec long)
                        (st_mtim.tv_nsec long)
                        (st_ctim.tv_sec long)
                        (st_ctim.tv_nsec long)))
(define (file-info fname/port follow?)
  (when (port? fname/port)
    (error "file-info implementation does not support ports as arguments"))
  (let* ((fname* (string->c-bytevector fname/port))
         (stat* (make-c-bytevector (c-type-size stat-struct)))
         (result (if follow?
                   (c-stat fname* stat*)
                   (c-lstat fname* stat*))))
    (when (< result 0)
      (let* ((error-message "file-info error")
             (error-msg* (string->c-bytevector error-message)))
        (c-perror error-msg*)
        (c-bytevector-free fname* stat* error-msg*)
        (error error-message fname/port)))
    (let ((file-info (make-file-info
                (c-bytevector-ref stat* stat-struct 'st_dev)
                (c-bytevector-ref stat* stat-struct 'st_ino)
                (c-bytevector-ref stat* stat-struct 'st_mode)
                (c-bytevector-ref stat* stat-struct 'st_nlink)
                (c-bytevector-ref stat* stat-struct 'st_uid)
                (c-bytevector-ref stat* stat-struct 'st_gid)
                (c-bytevector-ref stat* stat-struct 'st_rdev)
                (c-bytevector-ref stat* stat-struct 'st_size)
                (c-bytevector-ref stat* stat-struct 'st_blksize)
                (c-bytevector-ref stat* stat-struct 'st_blocks)
                (make-time time-utc
                           (c-bytevector-ref stat* stat-struct 'st_atim.tv_sec)
                           (c-bytevector-ref stat* stat-struct 'st_atim.tv_nsec))
                (make-time time-utc
                           (c-bytevector-ref stat* stat-struct 'st_mtim.tv_sec)
                           (c-bytevector-ref stat* stat-struct 'st_mtim.tv_nsec))
                (make-time time-utc
                           (c-bytevector-ref stat* stat-struct 'st_ctim.tv_sec)
                           (c-bytevector-ref stat* stat-struct 'st_ctim.tv_nsec))
                fname/port
                follow?)))
      (c-bytevector-free fname* stat*)
      file-info)))

(define create-directory
  (lambda (fname . permission-bits)
    (let* ((fname* (string->c-bytevector fname))
           (mode (if (null? permission-bits)
                   #o775
                   (string->number
                     (string-append
                       "#o"
                       (number->string (car permission-bits))))))
           (result (c-mkdir fname* mode))
           (error-message "create-directory error")
           (error-msg* (string->c-bytevector error-message)))
      (c-bytevector-free fname*)
      (when (< result 0)
        (c-perror error-msg*)
        (c-bytevector-free error-msg*)
        (error error-message))
      (c-bytevector-free error-msg*))))

(define (create-fifo fname . permission-bits)
  (let* ((fname* (string->c-bytevector fname))
         (mode (if (null? permission-bits)
                 #o664
                 (string->number
                   (string-append
                     "#o"
                     (number->string (car permission-bits))))))
         (result (c-mkfifo fname* mode))
         (error-message "create-fifo error")
         (error-msg* (string->c-bytevector error-message)))
    (c-bytevector-free fname*)
    (when (< result 0)
      (c-perror error-msg*)
      (c-bytevector-free error-msg*)
      (error error-message))
    (c-bytevector-free error-msg*)))

(define (create-hard-link old-fname new-fname)
  (let ((old-fname* (string->c-bytevector old-fname))
        (new-fname* (string->c-bytevector new-fname)))
  (c-link old-fname* new-fname*)
  (c-bytevector-free old-fname* new-fname*)))

(define (create-symlink old-fname new-fname)
  (c-slink (string->c-bytevector old-fname)
           (string->c-bytevector new-fname)))

(define (internal-read-symlink fname buffer-length)
  (let* ((path* (string->c-bytevector fname))
         (buffer (make-c-bytevector buffer-length))
         (result (c-readlink path* buffer (- buffer-length 1)))
         (error-message "read-symlink error")
         (error-msg* (string->c-bytevector error-message)))
    (cond ((< result 0)
           (c-perror error-msg*)
           (c-bytevector-free error-msg*)
           (error error-message))
          ((> result buffer-length)
           (c-bytevector-free path*)
           (c-bytevector-free buffer)
           (internal-read-symlink fname (+ buffer-length buffer-length)))
          (else
            (c-bytevector-set! buffer 'u8 result null-byte)
            (let ((name (c-bytevector->string buffer)))
              (c-bytevector-free path*)
              (c-bytevector-free buffer)
              name)))))

(define (read-symlink fname) (internal-read-symlink fname 128))

(define (rename-file old-fname new-fname)
  (c-rename (string->c-bytevector old-fname)
            (string->c-bytevector new-fname)))

(define (delete-directory fname)
  (let* ((fname* (string->c-bytevector fname))
         (result (c-rmdir fname*)))
    (c-bytevector-free fname*)
    (when (< result 0)
      (let* ((error-message "delete-directory error")
             (error-msg* (string->c-bytevector error-message)))
        (c-perror error-msg*)
        (c-bytevector-free error-msg*)
        (error error-message)))))

(define (set-file-owner fname uid gid)
  (let ((fname* (string->c-bytevector fname)))
    (c-chown fname* uid gid)
    (c-bytevector-free fname*)))

(define-c-array-type timespec-array 'long)
(define (set-file-times fname . args)
  (when (and (not (= (length args) 0))
             (not (= (length args) 2)))
    (error
      (string-append "set-file-times error: "
                     "It is an error if exactly one time is provided")))
  (let* ((current-time (posix-time))
         (access-time-object (if (null? args)
                               current-time
                               (car args)))
         (modify-time-object (if (or (null? args)
                                     (< (length args) 2))
                               current-time
                               (cadr args)))
         (fname-cbv (string->c-bytevector fname))
         (timespecs-cbv (make-c-bytevector (c-type-size* 'long 4)))
         (current-dir-cbv (string->c-bytevector (current-directory)))
         (current-dir-stream (c-opendir current-dir-cbv))
         (current-dir-fd (c-dirfd current-dir-stream)))
    (c-bytevector-set!
      timespecs-cbv timespec-array 0 (time-second access-time-object))
    (c-bytevector-set!
      timespecs-cbv timespec-array 1 (time-nanosecond access-time-object))
    (c-bytevector-set!
      timespecs-cbv timespec-array 2 (time-second modify-time-object))
    (c-bytevector-set!
      timespecs-cbv timespec-array 3 (time-nanosecond modify-time-object))
    (let ((result (c-utimensat current-dir-fd fname-cbv timespecs-cbv 0)))
      (c-bytevector-free fname-cbv timespecs-cbv current-dir-cbv current-dir-stream)
      (when (< result 0)
        (let* ((error-message "set-file-times error")
               (error-msg*(string->c-bytevector error-message)))
          (c-perror error-msg*)
          (c-bytevector-free error-msg*)
          (error error-message))))))

(define (truncate-file fname/port len)
  (when (not (exact-integer? len))
    (error "truncate-file error: len must be exact-integer"))
  (when (not (string? fname/port))
    (error "truncate-file error: ports not supported yet"))
  (let* ((fname/port-cbv (string->c-bytevector fname/port))
         (result (c-truncate fname/port-cbv len)))
    (c-bytevector-free fname/port-cbv)
    (when (< result 0)
      (let* ((error-message "truncate-file error")
             (error-msg* (string->c-bytevector error-message)))
        (c-perror error-msg*)
        (c-bytevector-free error-msg*)
        (error error-message)))))

(define (pointer-string-read pointer offset)
  (letrec* ((looper (lambda (c index result)
                      (if (char=? c #\null)
                        (list->string (reverse result))
                        (looper (c-bytevector-ref pointer
                                                  'char
                                                  (+ offset index))
                                (+ index 1)
                                (cons c result))))))
    (looper (c-bytevector-ref pointer 'char offset) 1 (list))))

; struct dirent d_name offset on linux
(define d-name-offset 19)

(define directory-files
  (lambda (dir . dotfiles?)
    (letrec* ((include-dotfiles? (if (null? dotfiles?) #f (car dotfiles?)))
              (path* (string->c-bytevector dir))
              (directory* (c-opendir path*))
              (error-message "directory-files error")
              (error-msg* (string->c-bytevector error-message))
              (dotfile? (lambda (name) (char=? (string-ref name 0) #\.)))
              (looper (lambda (directory-entity files)
                        (if (c-bytevector-null? directory-entity)
                          files
                          (let ((name (pointer-string-read directory-entity
                                                           d-name-offset)))
                            (looper (c-readdir directory*)
                                    (cond ((string=? name ".") files)
                                          ((string=? name "..") files)
                                          ((and include-dotfiles?
                                                (dotfile? name))
                                           (cons name files))
                                          ((not (dotfile? name))
                                           (cons name files))
                                          (else files))))))))
      (when (c-bytevector-null? directory*)
        (c-perror error-msg*)
        ;(c-bytevector-free error-msg*)
        ;(c-bytevector-free directory*)
        ;(c-bytevector-free path*)
        (error error-message))
      (let ((files (looper (c-readdir directory*) (list))))
        ;(c-bytevector-free error-msg*)
        ;(c-bytevector-free directory*)
        ;(c-bytevector-free path*)
        (c-closedir directory*)
        files))))

(define (set-file-mode path mode)
  (c-chmod (string->c-bytevector path)
           (string->number (string-append "#o" (number->string mode)))))

(define-record-type <directory>
  (make-directory handle dot-files?)
  directory?
  (handle directory:handle)
  (dot-files? directory:dot-files?))

(define (open-directory path . dot-files?)
  (make-directory (c-opendir (string->c-bytevector path))
                  (if (null? dot-files?)
                    #f
                    (car dot-files?))))

(define (read-directory directory-object)
  (let ((directory-entity (c-readdir (directory:handle directory-object))))
    (if (c-bytevector-null? directory-entity)
      (eof-object)
      (let ((name (pointer-string-read directory-entity d-name-offset)))
        (cond ((or (string=? name ".")
                   (string=? name ".."))
               (read-directory directory-object))
              ((and (directory:dot-files? directory-object)
                    (char=? (string-ref name 0) #\.))
               name)
              ((char=? (string-ref name 0) #\.)
               (read-directory directory-object))
              (else name))))))

(define (close-directory directory-object)
  (c-closedir (directory:handle directory-object)))

(define real-path
  (lambda (path)
    (let* ((path* (string->c-bytevector path))
           (real-path* (c-realpath path* (c-bytevector-null)))
           (real-path (string-copy (c-bytevector->string real-path*))))
      (c-bytevector-free path*)
      (c-bytevector-free real-path*)
      real-path)))

(define temp-file-prefix
  (make-parameter
    (if (get-environment-variable "TMPDIR")
      (string-append (get-environment-variable "TMPDIR")
                     slash
                     (number->string (c-getpid)))
      (string-append
        (cond-expand (windows (get-environment-variable "TMP")) (else "/tmp"))
        slash
        (number->string (c-getpid))))))

(define create-temp-file
  (lambda prefix
    (let* ((tmpdir (cond-expand
                     (windows (get-environment-variable "TMP"))
                     (else "/tmp")))
           (real-prefix
             (if (null? prefix)
               (string-append tmpdir slash (number->string (c-getpid)))
               (car prefix)))
           (path (string-append real-prefix "-" (random-string 6))))
      (if (file-exists? path)
        (create-temp-file real-prefix)
        (begin
          (with-output-to-file path (lambda () (display "")))
          (set-file-mode path 600)
          path)))))

(define (call-with-temporary-filename maker . prefix)
  (let* ((tmpdir (cond-expand (windows (get-environment-variable "TMP"))
                              (else "/tmp")))
         (real-prefix (if (null? prefix)
                        (string-append tmpdir
                                       slash
                                       (number->string (c-getpid)))
                        (car prefix)))
         (path (string-append real-prefix "-" (random-string 6))))
    (apply maker (list path))))

(define (umask)
  (let ((mask (c-umask 0)))
    (c-umask mask)
    mask))

(define (set-umask! umask)
  (c-umask umask))

(define (current-directory)
  (let* ((path* (make-c-bytevector 1024))
         (path (begin
                 (c-getcwd path* 1024)
                 (string-copy (c-bytevector->string path*)))))
    (c-bytevector-free path*)
    path))

(define (set-current-directory! path)
  (c-chdir (string->c-bytevector path)))

(define (pid) (c-getpid))

(define nice
  (lambda args
    (let ((result (if (null? args) (c-nice 1) (c-nice (car args)))))
      (when (< result 0)
        (let* ((error-message "nice error")
               (error-msg* (string->c-bytevector error-message)))
          (c-perror error-msg*)
          (c-bytevector-free timespec)
          (c-bytevector-free error-msg*)
          (error error-message)))
      result)))
(define (user-uid) (c-getuid))
(define (user-gid) (c-getgid))
(define (user-effective-uid) (c-geteuid))
(define (user-effective-gid) (c-getegid))

(define (groups-loop max-count count groups* result)
  (if (>= count max-count)
    result
    (groups-loop max-count
                 (+ count 1)
                 groups*
                 (append result
                         (list (c-bytevector-ref groups*
                                                 'int
                                                 (* (c-type-size 'int) count)))))))

(define (user-supplementary-gids)
  (let* ((group-count (c-getgroups 0 (c-bytevector-null)))
         (groups (make-c-bytevector (* (c-type-size 'int) group-count))))
    (c-getgroups group-count groups)
    (groups-loop group-count 0 groups (list))))

(define-record-type <user-info>
  (make-user-info name uid gid home-dir shell full-name)
  user-info?
  (name internal-user-info:name)
  (uid internal-user-info:uid)
  (gid internal-user-info:gid)
  (home-dir internal-user-info:home-dir)
  (shell internal-user-info:shell)
  (full-name internal-user-info:full-name))

(define (user-info:name user-info) (internal-user-info:name user-info))
(define (user-info:uid user-info) (internal-user-info:uid user-info))
(define (user-info:gid user-info) (internal-user-info:gid user-info))
(define (user-info:home-dir user-info) (internal-user-info:home-dir user-info))
(define (user-info:shell user-info) (internal-user-info:shell user-info))

(define (user-info:full-name user-info)
  (internal-user-info:full-name user-info))

(define (user-info:parsed-full-name user-info)
  (let* ((parsed-list
           (string-split (internal-user-info:full-name user-info) #\,))
         (first
           (string-append
             (string (char-upcase (string-ref (car parsed-list) 0)))
             (string-copy (car parsed-list) 1))))
    (cons (string-char-replace first #\& (user-info:name user-info))
          (cdr parsed-list))))

(define (user-info uid/name)
  (let ((password-struct (if (number? uid/name)
                           (c-getpwuid uid/name)
                           (let*
                             ((uid/name* (string->c-bytevector uid/name))
                              (result (c-getpwnam uid/name*)))
                             (c-bytevector-free uid/name*)
                             result))))
    (make-user-info (c-bytevector->string (c-bytevector-ref password-struct
                                                            'pointer
                                                            0))
                    (c-bytevector-ref password-struct
                                      'int
                                      (* (c-type-size 'pointer) 2))
                    (c-bytevector-ref password-struct
                                      'int
                                      (+ (* (c-type-size 'pointer) 2)
                                         (c-type-size 'int)))
                    (c-bytevector->string (c-bytevector-ref password-struct
                                                            'pointer
                                                            (+ (* (c-type-size 'pointer) 3)
                                                               (* (c-type-size 'int) 2))))
                    (c-bytevector->string (c-bytevector-ref password-struct
                                                            'pointer
                                                            (+ (* (c-type-size 'pointer) 4)
                                                               (* (c-type-size 'int) 2))))
                    (c-bytevector->string (c-bytevector-ref password-struct
                                                            'pointer
                                                            (+ (* (c-type-size 'pointer) 2)
                                                               (* (c-type-size 'int) 2)))))))


(define-record-type <group-info>
  (make-group-info name gid)
  group-info?
  (name group-info:name)
  (gid group-info:gid))

(define (group-info gid/name)
  (let ((group-struct (if (number? gid/name)
                        (c-getgrgid gid/name)
                        (c-getgrnam (string->c-bytevector gid/name)))))
    (make-group-info
      (c-bytevector->string (c-bytevector-ref group-struct 'pointer 0))
      (c-bytevector-ref group-struct
                        'int
                        (* (c-type-size 'pointer) 2)))))

(define (set-environment-variable! name value)
  (when (not (string? name))
    (error "set-environment-variable! error: name must be string"))
  (when (not (string? value))
    (error "set-environment-variable! error: value must be string"))
  (let ((name* (string->c-bytevector name))
        (value* (string->c-bytevector value)))
  (c-setenv name* value* 1)
  (c-bytevector-free name* value*)))

(define (delete-environment-variable! name)
  (when (not (string? name))
    (error "delete-environment-variable! error: Name must be string"))
  (c-unsetenv (string->c-bytevector name)))

(define CLOCK_REALTIME 0)
(define CLOCK_MONOTONIC 1)
(define tv_sec-type 'long)
(define tv_nsec-type 'long)
(define timespec (make-c-bytevector (c-type-size+ tv_sec-type tv_nsec-type)))

(define (posix-time)
  (let* ((result (c-clock-gettime CLOCK_REALTIME timespec)))
    (cond
      ((< result 0)
       (let* ((error-message "posix-time error")
              (error-msg* (string->c-bytevector error-message)))
         (c-perror error-msg*)
         (c-bytevector-free timespec)
         (c-bytevector-free error-msg*)
         (error error-message)))
      (else
        (make-time time-utc
                   (c-bytevector-ref timespec
                                     tv_nsec-type
                                     (c-type-size tv_sec-type))
                   (c-bytevector-ref timespec tv_sec-type 0))))))

(define (monotonic-time)
  (let* ((result (c-clock-gettime CLOCK_MONOTONIC timespec)))
    (cond
      ((< result 0)
       (let* ((error-message "posix-time error")
              (error-msg* (string->c-bytevector error-message)))
         (c-perror error-msg*)
         (c-bytevector-free timespec)
         (c-bytevector-free error-msg*)
         (error error-message)))
      (else
        (make-time time-utc
                   (c-bytevector-ref timespec
                                     tv_nsec-type
                                     (c-type-size tv_sec-type))
                   (c-bytevector-ref timespec tv_sec-type 0))))))

