;; Patina's portable directory API, backed by the configured VFS.
;; See docs/VFS_DESIGN.md for the public contract and filesystem boundaries.
(define-library (patina filesystem)
  (import (only (patina internal io)
                directory-files create-directory delete-directory
                current-directory change-directory
                file-directory? file-regular?))
  (export directory-files create-directory delete-directory
          current-directory change-directory
          file-directory? file-regular?))
