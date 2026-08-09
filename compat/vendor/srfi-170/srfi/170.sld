(define-library
  (srfi 170)
  (import (scheme base)
          (scheme char)
          (scheme write)
          (scheme file)
          (scheme process-context)
          (foreign c)
          (srfi 19))
  (export ;posix-error?
          ;posix-error-name
          ;posix-error-message
          ;open-file
          ;fd->port
          create-directory
          create-fifo
          create-hard-link
          create-symlink
          read-symlink
          rename-file
          delete-directory
          set-file-owner
          set-file-times
          truncate-file
          file-info
          file-info?
          file-info:device
          file-info:inode
          file-info:mode
          file-info:nlinks
          file-info:uid
          file-info:gid
          file-info:rdev
          file-info:size
          file-info:blksize
          file-info:blocks
          file-info:atime
          file-info:mtime
          file-info:ctime
          file-info-directory?
          ;file-info-fifo?
          ;file-info-symlink?
          ;file-info-regular?
          ;file-info-socket?
          ;file-info-device?
          set-file-mode
          directory-files
          ;make-directory-files-generator
          open-directory
          read-directory
          close-directory
          real-path
          ;file-space
          temp-file-prefix
          create-temp-file
          call-with-temporary-filename
          umask
          set-umask!
          current-directory
          set-current-directory!
          pid
          nice
          user-uid
          user-gid
          user-effective-uid
          user-effective-gid
          user-supplementary-gids
          user-info
          user-info?
          user-info:name
          user-info:uid
          user-info:gid
          user-info:home-dir
          user-info:shell
          user-info:full-name
          user-info:parsed-full-name
          group-info
          group-info?
          group-info:name
          group-info:gid
          posix-time
          monotonic-time
          set-environment-variable!
          delete-environment-variable!
          ;terminal?
          )
  (include "170.scm"))
