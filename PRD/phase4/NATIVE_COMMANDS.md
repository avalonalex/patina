# Native Commands in Patina

Implementing common Unix commands as native Scheme functions for better composability.

## Philosophy

Instead of always shelling out, implement common operations **natively in Scheme**:

```scheme
;; Bad: Shell out, get string
(shell "ls -la")  ; => string output

;; Good: Native function, get structured data
(ls #:long #t #:all #t)  ; => list of file-info records

;; Even better: Compose with Scheme
(filter (lambda (f) (> (file-info-size f) 1000))
        (ls #:all #t))  ; Files > 1KB
```

## Three-Tier Design

### Tier 1: Native Scheme (Preferred)
Common operations implemented in pure Scheme with structured output.

### Tier 2: Tabular API
Commands that produce table-like data with consistent interface.

### Tier 3: Shell Catch-all
Generic shell execution for everything else.

## Tier 1: Native Commands

### File System

```scheme
(library (patina fs)
  (export
    ls directory-list          ; List directory
    tree                       ; Directory tree
    file-info                  ; File metadata
    find                       ; Find files
    du                         ; Disk usage
    ))

;; ls - returns list of file-info records
(define-record-type file-info
  (fields name
          type        ; 'file | 'directory | 'symlink
          size        ; bytes
          permissions ; permissions record
          modified    ; timestamp
          owner))

(define (ls #:key
            (path ".")
            (all #f)
            (long #f)
            (recursive #f)
            (pattern #f))
  "List directory contents as structured data"
  (let ((entries (directory-entries path all)))
    (if pattern
        (filter (lambda (e) (glob-match? pattern (file-info-name e)))
                entries)
        entries)))

;; Usage examples
(ls)                           ; List current directory
(ls #:all #t)                  ; Include hidden
(ls #:pattern "*.scm")         ; Pattern matching
(ls #:path "/tmp" #:long #t)   ; With full info

;; Compose with Scheme!
(length (ls))                  ; Count files

(map file-info-name            ; Get just names
     (ls))

(filter (lambda (f)            ; Files > 1MB
          (> (file-info-size f) (* 1024 1024)))
        (ls #:all #t))

(apply +                       ; Total size
       (map file-info-size (ls)))
```

### Directory Tree

```scheme
(define-record-type tree-node
  (fields path
          info
          children))  ; list of tree-nodes

(define (tree #:key
              (path ".")
              (max-depth #f)
              (pattern #f))
  "Build directory tree as structured data"
  (build-tree-node path max-depth pattern))

;; Usage
(define project-tree (tree #:max-depth 3))

;; Navigate the tree
(tree-node-children project-tree)

;; Print it
(define (print-tree node #:indent 0)
  (display (make-string indent #\space))
  (display (file-info-name (tree-node-info node)))
  (newline)
  (for-each (lambda (child)
              (print-tree child #:indent (+ indent 2)))
            (tree-node-children node)))

(print-tree project-tree)
```

### File Operations

```scheme
(library (patina fs))
  (export
    ; Queries
    file-exists? directory-exists?
    file-size file-type
    file-modified file-permissions

    ; Operations
    copy move delete
    make-directory make-directories

    ; Reading/Writing
    read-file read-lines
    write-file append-file

    ; Permissions
    chmod chown))

;; Native implementations
(define (file-size path)
  "Get file size in bytes"
  (file-info-size (file-stat path)))

(define (file-type path)
  "Get file type: 'file, 'directory, 'symlink"
  (file-info-type (file-stat path)))

(define (read-file path)
  "Read entire file as string"
  (call-with-input-file path
    (lambda (port) (read-string port))))

(define (read-lines path)
  "Read file as list of lines"
  (call-with-input-file path
    (lambda (port)
      (let loop ((lines '()))
        (let ((line (read-line port)))
          (if (eof-object? line)
              (reverse lines)
              (loop (cons line lines))))))))

(define (write-file path content)
  "Write string to file"
  (call-with-output-file path
    (lambda (port) (write-string content port))))

;; Usage - pure Scheme, no shell!
(define files (ls))
(define large-files
  (filter (lambda (f) (> (file-size (file-info-name f)) 1000))
          files))

(write-file "report.txt"
  (format "Found ~a large files" (length large-files)))
```

### Find

```scheme
(define (find path #:key
              (name #f)
              (type #f)
              (size #f)
              (modified #f)
              (max-depth #f))
  "Find files matching criteria"
  (let search ((dir path) (depth 0) (results '()))
    (if (and max-depth (> depth max-depth))
        results
        (let ((entries (ls #:path dir #:all #t)))
          (fold-left
            (lambda (acc entry)
              (let ((entry-path (string-append dir "/" (file-info-name entry)))
                    (matches?
                      (and (or (not name) (glob-match? name (file-info-name entry)))
                           (or (not type) (eq? type (file-info-type entry)))
                           (or (not size) ((car size) (file-info-size entry) (cdr size))))))
                (if matches?
                    (cons entry-path acc)
                    acc)))
            results
            entries)))))

;; Usage
(find "." #:name "*.scm")                    ; All Scheme files
(find "/src" #:type 'file #:size (cons > 1000))  ; Files > 1000 bytes
(find "." #:max-depth 2 #:name "test*")      ; Max depth 2
```

## Tier 2: Tabular API

For commands that produce table-like output, provide a consistent interface:

```scheme
(library (patina table)
  (export
    table? make-table
    table-headers table-rows
    table-ref table-filter
    table-map table-sort
    table->csv table->markdown
    display-table))

(define-record-type table
  (fields headers  ; List of symbols
          rows))   ; List of lists

;; ps - Process listing
(define (ps #:key (all #f))
  "Get process list as table"
  (make-table
    '(pid user cpu mem command)
    (get-process-list all)))

;; Usage
(define processes (ps #:all #t))

;; Filter
(table-filter processes
  (lambda (row)
    (> (list-ref row 2) 50)))  ; CPU > 50%

;; Sort
(table-sort processes 'cpu #:reverse #t)

;; Display
(display-table processes)
```

### Common Commands as Tables

```scheme
;; df - Disk usage
(define (df)
  "Filesystem disk usage as table"
  (make-table
    '(filesystem size used available use% mount)
    (get-filesystem-info)))

;; netstat - Network connections
(define (netstat)
  "Network connections as table"
  (make-table
    '(proto local-addr foreign-addr state)
    (get-network-connections)))

;; git log
(define (git-log #:key (max-count 10))
  "Git log as table"
  (make-table
    '(hash author date message)
    (parse-git-log max-count)))

;; Usage - composable!
(define usage (df))

;; Find filesystems over 80% full
(table-filter usage
  (lambda (row)
    (let ((use-percent (parse-percentage (list-ref row 4))))
      (> use-percent 80))))

;; Get just mount points
(table-map usage 'mount)

;; Export to CSV
(table->csv usage "disk-usage.csv")
```

### Table Operations

```scheme
;; Filtering
(define (table-filter tbl pred)
  "Filter rows by predicate"
  (make-table
    (table-headers tbl)
    (filter pred (table-rows tbl))))

;; Mapping
(define (table-map tbl column)
  "Extract a single column"
  (let ((idx (list-index (lambda (h) (eq? h column))
                        (table-headers tbl))))
    (map (lambda (row) (list-ref row idx))
         (table-rows tbl))))

;; Sorting
(define (table-sort tbl column #:key (reverse #f))
  "Sort table by column"
  (let ((idx (list-index (lambda (h) (eq? h column))
                        (table-headers tbl))))
    (make-table
      (table-headers tbl)
      (sort (table-rows tbl)
            (lambda (a b)
              (let ((cmp (compare (list-ref a idx)
                                (list-ref b idx))))
                (if reverse (not cmp) cmp)))))))

;; Joining
(define (table-join tbl1 tbl2 column)
  "Join two tables on column"
  ...)

;; Display
(define (display-table tbl #:key (max-rows #f))
  "Pretty print table"
  (let* ((headers (table-headers tbl))
         (rows (if max-rows
                   (take (table-rows tbl) max-rows)
                   (table-rows tbl)))
         (widths (compute-column-widths headers rows)))
    (print-header headers widths)
    (print-separator widths)
    (for-each (lambda (row)
                (print-row row widths))
              rows)))
```

## Tier 3: Shell Catch-all

For everything else, generic shell execution:

```scheme
(library (patina shell)
  (export
    shell           ; Execute command
    $               ; Alias
    sh              ; Alias
    shell-capture   ; Capture output
    shell-lines     ; Output as lines
    shell-check     ; Check exit status
    pipe            ; Pipe commands
    ))

;; Generic shell for uncommon commands
(shell "ffmpeg -i input.mp4 output.avi")
(shell "docker ps")
(shell "kubectl get pods")

;; But prefer native when available!
(ls) instead of (shell "ls")
(ps) instead of (shell "ps")
```

## API Design Patterns

### Pattern 1: Keywords for Options

```scheme
;; Clear, self-documenting
(ls #:all #t #:long #t #:pattern "*.scm")

;; Not: (ls "-la" "*.scm")  ; Unix-style flags are opaque
```

### Pattern 2: Structured Return Values

```scheme
;; Good: Returns records
(define files (ls))
(file-info-size (car files))  ; => 1024

;; Bad: Returns strings
(define output (shell "ls -l"))  ; => "total 8\n-rw-r--r--..."
```

### Pattern 3: Composability

```scheme
;; Chain operations naturally
(filter (lambda (f) (> (file-info-size f) 1000))
        (ls #:all #t))

;; Pipe-like composition
(apply +
  (map file-info-size
    (filter (lambda (f) (eq? (file-info-type f) 'file))
            (ls #:recursive #t))))
```

## Implementation Priority

### Phase 1: Essential File Operations
```scheme
(library (patina fs))
  ls              ; ✅ High priority
  file-info       ; ✅ High priority
  read-file       ; ✅ High priority
  write-file      ; ✅ High priority
  find            ; ⚠️  Medium
  tree            ; ⚠️  Medium
```

### Phase 2: Process & System
```scheme
(library (patina system))
  ps              ; Table API
  df              ; Table API
  top             ; Interactive
  env             ; Environment vars
```

### Phase 3: Developer Tools
```scheme
(library (patina git))
  git-log         ; Table API
  git-status      ; Structured
  git-diff        ; Structured
  git-blame       ; Table API
```

## Example: Native vs Shell

```scheme
;; ❌ Shell-only approach (Jupyter style)
(define files (shell-lines "ls *.scm"))
(define sizes
  (map (lambda (f)
         (string->number
           (car (shell-lines (string-append "stat -f%z " f)))))
       files))

;; ✅ Native approach (Patina style)
(define files
  (ls #:pattern "*.scm"))

(define sizes
  (map file-info-size files))

;; So much cleaner!
```

## Example: Complex Analysis

```scheme
(cell code
  (import (patina fs)
          (patina table))

  ;; Find large Scheme files
  (define large-scheme-files
    (filter (lambda (f)
              (and (> (file-info-size f) 1000)
                   (glob-match? "*.scm" (file-info-name f))))
            (find "src" #:type 'file #:recursive #t)))

  ;; Create summary table
  (define summary
    (make-table
      '(file size lines)
      (map (lambda (f)
             (list (file-info-name f)
                   (file-info-size f)
                   (length (read-lines (file-info-name f)))))
           large-scheme-files)))

  ;; Display sorted by size
  (display-table
    (table-sort summary 'size #:reverse #t)))
```

## Benefits

### 1. Type Safety
```scheme
(file-info-size f)  ; => integer, guaranteed

vs

(string->number     ; => might fail!
  (shell-capture "stat ..."))
```

### 2. Cross-Platform
```scheme
(ls)  ; Works on Unix, Windows, etc.

vs

(shell "ls")  ; Unix only!
```

### 3. Performance
```scheme
(ls)  ; Fast, in-process

vs

(shell "ls")  ; Spawn process, parse output
```

### 4. Composability
```scheme
(map file-info-size (ls))  ; Natural composition

vs

(map parse-size            ; Manual parsing needed
     (string-split (shell "ls -l") #\newline))
```

## Summary

**Three-tier design:**

1. **Native Scheme** (Tier 1)
   - Common operations: ls, file ops, find
   - Returns structured data (records, lists)
   - Fast, cross-platform, composable

2. **Tabular API** (Tier 2)
   - Commands with table output: ps, df, git-log
   - Consistent table interface
   - Filtering, sorting, joining

3. **Shell Catch-all** (Tier 3)
   - Generic `shell` for everything else
   - Necessary fallback
   - Use sparingly

**Result:** Better than both Unix pipes AND Jupyter's magic commands!
