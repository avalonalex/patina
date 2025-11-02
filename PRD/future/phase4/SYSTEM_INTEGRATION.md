# System Integration in Patina

How to elegantly integrate shell commands and system operations into Scheme, making Jupyter's `!` and `%` syntax obsolete.

## Philosophy: Everything is a Function

In Jupyter:
```python
# Awkward magic syntax
!ls -la
%cd /tmp
%system git status

# Mixing languages poorly
result = !cat file.txt
```

In Patina:
```scheme
; Clean, composable functions
(shell "ls -la")
(cd "/tmp")
(git 'status)

; Proper return values
(define result (shell "cat file.txt" #:capture #t))
```

## Three-Tier Architecture

Patina uses a **three-tier approach** to system integration:

### Tier 1: Native Scheme Commands (Preferred)
Common operations implemented in pure Scheme with structured output:
```scheme
(library (patina fs)
  (export
    ls tree find           ; Returns structured data (records)
    file-info file-size    ; Type-safe accessors
    read-file read-lines   ; Native I/O
    write-file copy move   ; File operations
    ))
```

### Tier 2: Tabular API
Commands that produce table-like data:
```scheme
(library (patina table)
  (export
    ps df netstat git-log  ; Returns table records
    table-filter           ; Filter rows
    table-sort             ; Sort by column
    table-map              ; Extract columns
    display-table          ; Pretty print
    ))
```

### Tier 3: Shell Catch-all
Generic shell execution for everything else:
```scheme
(library (patina shell)
  (export
    shell sh $             ; Execute command
    shell-capture          ; Capture output
    pipe                   ; Unix pipes
    ))
```

See [NATIVE_COMMANDS.md](NATIVE_COMMANDS.md) for complete specification.

## Why Three Tiers?

**Example: Listing files**

```scheme
;; ❌ Tier 3 only (shell) - returns string
(shell "ls -la")  ; => "total 8\n-rw-r--r-- ..."

;; ✅ Tier 1 (native) - returns structured data
(ls #:long #t #:all #t)  ; => list of file-info records

;; Now composable!
(filter (lambda (f) (> (file-info-size f) 1000))
        (ls #:all #t))  ; Files > 1KB
```

## Core System Library

```scheme
(library (patina system)
  (import (patina fs)
          (patina table)
          (patina shell))
  (export
    ; Tier 1: Native commands
    ls tree find              ; Structured file operations
    file-info file-size       ; Type-safe file queries
    read-file read-lines      ; Native I/O
    write-file copy move      ; File operations

    ; Tier 2: Tabular commands
    ps df netstat             ; System info as tables
    git-log git-blame         ; Git as tables
    table-filter table-sort   ; Table operations

    ; Tier 3: Shell catch-all
    shell sh $                ; Generic shell
    shell-capture             ; Capture output
    pipe                      ; Unix pipes

    ; Process management
    process-start             ; Start process
    process-wait              ; Wait for completion
    process-kill              ; Terminate

    ; Environment
    getenv setenv             ; Environment vars
    which                     ; Find executable
    home-directory            ; User home

    ; Advanced
    with-directory            ; Temporary cd
    with-environment          ; Temporary env
    parallel                  ; Parallel execution
    ))
```

## Implementation Examples

### Basic Shell Execution

```scheme
;; Simple execution
(define (shell cmd . args)
  "Execute shell command, return exit status"
  (let ((full-cmd (if (null? args)
                      cmd
                      (string-join (cons cmd args) " "))))
    (process-run full-cmd #:stdout (current-output-port))))

;; Usage
(shell "ls -la")           ; Display output
(shell "git status")       ; Show git status
```

### Capturing Output

```scheme
;; Capture as string
(define (shell-capture cmd)
  (call-with-port (open-input-pipe cmd)
    (lambda (port)
      (read-string port))))

;; Capture as lines
(define (shell-lines cmd)
  (string-split (shell-capture cmd) #\newline))

;; Usage
(define files (shell-lines "ls *.scm"))
(length files)  ; => 5

(define status (shell-capture "git status"))
(display status)
```

### Process Management

```scheme
;; Start background process
(define (process-start cmd #:key (stdout #f) (stderr #f))
  (let ((proc (spawn-process cmd)))
    (when stdout
      (redirect-output proc stdout))
    (when stderr
      (redirect-error proc stderr))
    proc))

;; Usage
(define server (process-start "python -m http.server"
                              #:stdout "server.log"))
(process-wait server 5)  ; Wait 5 seconds
(process-kill server)     ; Stop it
```

### Unix Pipes

```scheme
;; Composable pipes
(define-syntax pipe
  (syntax-rules ()
    ((pipe cmd)
     (shell-capture cmd))
    ((pipe cmd rest ...)
     (pipe-compose (shell-capture cmd) rest ...))))

;; Helper
(define (pipe-compose input . cmds)
  (fold-left
    (lambda (data cmd)
      (call-with-port
        (open-input-string data)
        (lambda (in-port)
          (call-with-port
            (open-output-string)
            (lambda (out-port)
              (parameterize ((current-input-port in-port)
                           (current-output-port out-port))
                (shell cmd))
              (get-output-string out-port))))))
    input
    cmds))

;; Usage - elegant Unix pipes!
(pipe
  "cat data.txt"
  "grep ERROR"
  "wc -l")
;; => "42\n"

;; Or with Scheme processing
(pipe
  "ls *.scm"
  (lambda (files)
    (filter (lambda (f) (string-contains? f "test"))
            (string-split files #\newline)))
  "sort")
```

### Git Integration

```scheme
;; Generic git command
(define (git command . args)
  (apply shell "git" (symbol->string command) args))

;; Convenience functions
(define (git-status)
  (git 'status))

(define (git-log #:key (max-count 10) (oneline #f))
  (git 'log
       (if oneline "--oneline" "")
       (format "--max-count=~a" max-count)))

(define (git-commit message)
  (git 'commit "-m" message))

(define (git-diff . files)
  (apply git 'diff files))

;; Usage
(git-status)
(git-log #:max-count 5 #:oneline #t)
(git 'add ".")
(git-commit "Update analysis")
(git 'push "origin" "main")
```

### Directory Operations

```scheme
;; Change directory
(define current-directory (make-parameter (getcwd)))

(define (cd path)
  (change-directory path)
  (current-directory path))

(define (pwd)
  (current-directory))

;; List directory with filtering
(define (ls #:key (path ".") (pattern #f) (hidden #f))
  (let ((files (directory-files path)))
    (cond
      ((and pattern (not hidden))
       (filter (lambda (f)
                 (and (glob-match? pattern f)
                      (not (string-prefix? "." f))))
               files))
      (pattern
       (filter (lambda (f) (glob-match? pattern f)) files))
      ((not hidden)
       (filter (lambda (f) (not (string-prefix? "." f))) files))
      (else files))))

;; Usage
(ls)                           ; Current directory
(ls #:pattern "*.scm")         ; Scheme files
(ls #:path "/tmp" #:hidden #t) ; Include hidden
```

### Temporary Context

```scheme
;; Execute with temporary directory
(define-syntax with-directory
  (syntax-rules ()
    ((with-directory dir body ...)
     (let ((old-dir (current-directory)))
       (dynamic-wind
         (lambda () (cd dir))
         (lambda () body ...)
         (lambda () (cd old-dir)))))))

;; Usage
(with-directory "/tmp"
  (shell "ls -la")
  (define files (ls))
  (length files))
;; Back to original directory

;; Temporary environment
(define-syntax with-environment
  (syntax-rules ()
    ((with-environment ((var val) ...) body ...)
     (let ((old-env (map (lambda (v) (cons v (getenv v)))
                        '(var ...))))
       (dynamic-wind
         (lambda () (for-each (lambda (p) (setenv (car p) (cdr p)))
                            '((var . val) ...)))
         (lambda () body ...)
         (lambda () (for-each (lambda (p) (setenv (car p) (cdr p)))
                            old-env)))))))

;; Usage
(with-environment (("DEBUG" "1")
                   ("VERBOSE" "true"))
  (shell "./my-script.sh"))
;; Environment restored
```

## Advanced Patterns

### DSL for Shell Commands

```scheme
;; Create a shell DSL
(define-syntax define-shell-command
  (syntax-rules ()
    ((define-shell-command name cmd)
     (define (name . args)
       (apply shell cmd (map ->string args))))))

;; Define commands
(define-shell-command docker "docker")
(define-shell-command kubectl "kubectl")
(define-shell-command cargo "cargo")

;; Usage
(docker "ps")
(kubectl "get" "pods")
(cargo "build" "--release")

;; With keyword arguments
(define (cargo-build #:key (release #f) (features '()))
  (shell "cargo" "build"
         (if release "--release" "")
         (if (null? features)
             ""
             (format "--features=~a" (string-join features ",")))))

(cargo-build #:release #t #:features '("async" "tls"))
```

### Smart Command Wrapper

```scheme
;; Detect command location and provide nice errors
(define (which cmd)
  (shell-capture (string-append "which " cmd)))

(define (ensure-command cmd)
  (unless (file-exists? (string-trim (which cmd)))
    (error (format "Command not found: ~a" cmd))))

(define-syntax defcommand
  (syntax-rules ()
    ((defcommand name binary)
     (begin
       (ensure-command binary)
       (define (name . args)
         (apply shell binary (map ->string args)))))))

;; Usage
(defcommand terraform "terraform")
(defcommand ansible "ansible-playbook")

;; These will error nicely if tools aren't installed
(terraform "plan")
(ansible "deploy.yml")
```

### Parallel Execution

```scheme
;; Run commands in parallel
(define (parallel . commands)
  (let ((processes (map (lambda (cmd)
                         (process-start cmd #:capture #t))
                       commands)))
    (map process-wait processes)))

;; Usage
(parallel
  "cargo build"
  "npm run build"
  "python setup.py build")

;; With results
(define (parallel-capture . commands)
  (let* ((processes (map (lambda (cmd)
                          (process-start cmd #:capture #t))
                        commands))
         (results (map process-output processes)))
    results))

(define outputs
  (parallel-capture
    "git log --oneline -5"
    "git status"
    "git diff --stat"))
```

### Error Handling

```scheme
;; Check command success
(define (shell-check cmd)
  (let ((status (shell cmd)))
    (unless (zero? status)
      (error (format "Command failed with status ~a: ~a" status cmd)))
    status))

;; With retry
(define (shell-retry cmd #:max-attempts 3)
  (let retry ((attempts 0))
    (let ((status (shell cmd)))
      (if (zero? status)
          status
          (if (< attempts max-attempts)
              (begin
                (display (format "Retry ~a/~a...\n" (+ attempts 1) max-attempts))
                (retry (+ attempts 1)))
              (error (format "Command failed after ~a attempts" max-attempts)))))))

;; Usage
(shell-retry "curl https://api.example.com/data" #:max-attempts 5)
```

## Integration with Notebooks

### In Code Cells

```scheme
(cell code
  (import (patina system))

  ;; Run build
  (shell-check "cargo build --release")

  ;; Check results
  (define binary-size
    (string->number
      (shell-capture "stat -f%z target/release/patina")))

  (display (format "Binary size: ~a KB\n" (/ binary-size 1024))))
```

### Data Analysis Pipeline

```scheme
(cell code
  ;; Download data
  (shell "curl -s https://data.example.com/sales.csv > sales.csv")

  ;; Process with external tool
  (shell "csvkit filter sales.csv > filtered.csv")

  ;; Load into Scheme
  (define data (read-csv "filtered.csv"))

  ;; Analyze
  (define total (apply + (map cadr data)))

  ;; Commit results
  (git 'add "filtered.csv")
  (git-commit "Update analysis data"))
```

### DevOps Automation

```scheme
(cell code
  (import (patina system))

  ;; Build
  (shell-check "cargo build --release")

  ;; Test
  (shell-check "cargo test")

  ;; Deploy
  (with-environment (("ENVIRONMENT" "production"))
    (shell-check "./deploy.sh"))

  ;; Tag release
  (define version "v0.1.0")
  (git 'tag version)
  (git 'push "origin" version))
```

## Comparison Table

| Operation | Jupyter | Patina |
|-----------|---------|--------|
| Shell command | `!ls` | `(shell "ls")` |
| Capture output | `x = !cat file` | `(define x (shell-capture "cat file"))` |
| Directory change | `%cd /tmp` | `(cd "/tmp")` |
| Environment | `%env VAR=val` | `(setenv "VAR" "val")` |
| Git | `!git status` | `(git 'status)` |
| Pipes | N/A | `(pipe "cat" "grep" "wc")` |
| Parallel | N/A | `(parallel cmd1 cmd2)` |
| Error handling | Try/catch | `(shell-check cmd)` |
| Composability | ❌ Limited | ✅ Full |

## Why This is Better

### 1. **Consistent Syntax**
Everything is a function call. No special syntax to remember.

### 2. **Composable**
```scheme
;; Chain operations naturally
(define file-count
  (length
    (filter (lambda (f) (string-suffix? ".scm" f))
            (ls))))

;; Combine with Scheme
(define (analyze-project dir)
  (with-directory dir
    (let ((files (shell-lines "find . -name '*.scm'")))
      (map count-lines files))))
```

### 3. **Extensible**
Define your own abstractions:

```scheme
(define-syntax deployment-pipeline
  (syntax-rules (build test deploy)
    ((deployment-pipeline
       (build BUILD-CMD)
       (test TEST-CMD)
       (deploy DEPLOY-CMD))
     (begin
       (display "Building...\n")
       (shell-check BUILD-CMD)
       (display "Testing...\n")
       (shell-check TEST-CMD)
       (display "Deploying...\n")
       (shell-check DEPLOY-CMD)
       (display "Done!\n")))))

;; Use it
(deployment-pipeline
  (build "cargo build --release")
  (test "cargo test")
  (deploy "./deploy.sh"))
```

### 4. **Proper Error Handling**
```scheme
(guard (ex
  ((command-error? ex)
   (display "Build failed, rolling back...\n")
   (git 'checkout "HEAD~1")))
  (shell-check "make build")
  (shell-check "make deploy"))
```

### 5. **Type Safe**
Return proper Scheme values:

```scheme
(define-record-type git-status
  (fields modified added deleted))

(define (git-status-structured)
  (let ((output (shell-capture "git status --porcelain")))
    (parse-git-status output)))  ; Returns git-status record

;; Now you can:
(define status (git-status-structured))
(git-status-modified status)  ; List of modified files
```

## Future: tui-textarea Integration

When building the notebook TUI, use `tui-textarea` for cell editing:

```rust
// In notebook TUI
use tui_textarea::TextArea;

struct CodeCell {
    id: String,
    editor: TextArea<'static>,
    output: Option<Value>,
}

// Features we get:
// - Multi-line editing
// - Syntax highlighting
// - Vim keybindings
// - Yank/paste
// - Search
```

This gives us a proper code editor in each cell, better than Jupyter's CodeMirror!

## Conclusion

By treating system commands as first-class Scheme functions:
- No magic syntax (`!` or `%`)
- Full composability
- Proper error handling
- Extensible with macros
- Type-safe return values
- Natural integration with Scheme

The result is more elegant and more powerful than Jupyter's approach.
