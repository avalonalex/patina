# Three-Tier System Integration - Visual Summary

A quick reference for Patina's elegant system integration design.

## The Three Tiers

```
┌─────────────────────────────────────────────────────┐
│                  TIER 1: NATIVE                     │
│            Pure Scheme, Structured Data             │
│                                                     │
│  (ls) → list of file-info records                  │
│  (find) → list of paths                            │
│  (tree) → tree-node structure                      │
│  (read-file) → string                              │
│                                                     │
│  ✅ Type-safe  ✅ Fast  ✅ Cross-platform          │
├─────────────────────────────────────────────────────┤
│                  TIER 2: TABLES                     │
│          Tabular Data, Structured Interface         │
│                                                     │
│  (ps) → table with (pid user cpu mem cmd)          │
│  (df) → table with (fs size used available)        │
│  (git-log) → table with (hash author date msg)     │
│                                                     │
│  ✅ Filterable  ✅ Sortable  ✅ Exportable         │
├─────────────────────────────────────────────────────┤
│                  TIER 3: SHELL                      │
│            Catch-all for Everything Else            │
│                                                     │
│  (shell "ffmpeg -i in.mp4 out.avi")                │
│  (shell "docker ps")                               │
│  (shell "kubectl get pods")                        │
│                                                     │
│  ✅ Flexible  ⚠️ String output  ⚠️ Platform-dep    │
└─────────────────────────────────────────────────────┘
```

## Decision Tree: Which Tier?

```
Is this a common file operation?
├─ YES → Use Tier 1 (Native)
│         (ls), (find), (tree), (read-file)
│
├─ Does it produce tabular data?
│  ├─ YES → Use Tier 2 (Table API)
│  │         (ps), (df), (git-log), (netstat)
│  │
│  └─ NO → Use Tier 3 (Shell)
│            (shell "command")
```

## Comparison Example

### Listing Files

**Tier 3 (Shell):**
```scheme
(shell "ls -la")
;; => "total 16\ndrwxr-xr-x  4 user  staff  128 Oct 27\n..."
;;    ❌ String output - hard to parse
;;    ❌ Platform-specific
;;    ❌ Not composable
```

**Tier 1 (Native):**
```scheme
(ls #:long #t #:all #t)
;; => (#(file-info
;;         name: "file.txt"
;;         size: 1024
;;         type: 'file
;;         modified: <timestamp>)
;;      ...)
;;    ✅ Structured records
;;    ✅ Type-safe accessors
;;    ✅ Composable with Scheme
```

### Process Listing

**Tier 3 (Shell):**
```scheme
(shell "ps aux")
;; => "USER   PID  %CPU %MEM\nroot  1234  45.2  128\n..."
;;    ❌ Need to parse manually
;;    ❌ Fragile (format changes)
```

**Tier 2 (Table):**
```scheme
(ps #:all #t)
;; => #(table
;;       headers: (pid user cpu mem command)
;;       rows: ((1234 "root" 45.2 128 "scheme")
;;              (5678 "user" 12.1 64 "rust")
;;              ...))
;;    ✅ Structured table
;;    ✅ Built-in operations
;;    ✅ Consistent interface
```

## Composability Examples

### Tier 1: Native Composition

```scheme
;; Find large Scheme files
(filter (lambda (f)
          (and (> (file-info-size f) 1000)
               (glob-match? "*.scm" (file-info-name f))))
        (ls #:recursive #t))

;; Calculate total project size
(apply +
  (map file-info-size
    (find "src" #:type 'file)))

;; Read and process all Scheme files
(map (lambda (f)
       (cons (file-info-name f)
             (length (read-lines (file-info-name f)))))
     (find "." #:name "*.scm"))
```

### Tier 2: Table Operations

```scheme
;; Find high CPU processes
(table-filter (ps)
  (lambda (row)
    (> (list-ref row 2) 50)))  ; CPU > 50%

;; Sort by memory
(table-sort (ps) 'mem #:reverse #t)

;; Export to CSV
(table->csv (df) "disk-usage.csv")

;; Join tables
(table-join
  (git-log #:max-count 100)
  (git-blame "src/main.rs")
  'author)
```

### Tier 3: Shell + Scheme

```scheme
;; Shell output → Scheme processing
(define lines (shell-lines "git log --oneline"))
(length lines)  ; Number of commits

;; Pipe multiple commands
(pipe
  "cat access.log"
  "grep ERROR"
  "wc -l")
```

## API Reference Card

### Tier 1: Native Commands

| Function | Returns | Example |
|----------|---------|---------|
| `(ls #:pattern "*.scm")` | list of file-info | File listing |
| `(find "." #:type 'file)` | list of paths | Recursive search |
| `(tree #:max-depth 3)` | tree-node | Directory tree |
| `(file-info path)` | file-info record | File metadata |
| `(file-size path)` | integer | Size in bytes |
| `(read-file path)` | string | File contents |
| `(read-lines path)` | list of strings | Lines as list |
| `(write-file path data)` | unspecified | Write file |

### Tier 2: Table Commands

| Function | Returns | Example |
|----------|---------|---------|
| `(ps #:all #t)` | table | Process list |
| `(df)` | table | Filesystem usage |
| `(git-log #:max-count 10)` | table | Git commits |
| `(netstat)` | table | Network connections |
| `(table-filter tbl pred)` | table | Filter rows |
| `(table-sort tbl col)` | table | Sort by column |
| `(table-map tbl col)` | list | Extract column |
| `(display-table tbl)` | void | Pretty print |

### Tier 3: Shell Commands

| Function | Returns | Example |
|----------|---------|---------|
| `(shell "cmd")` | exit-code | Execute |
| `(shell-capture "cmd")` | string | Capture output |
| `(shell-lines "cmd")` | list | Lines as list |
| `(shell-check "cmd")` | void or error | Check success |
| `(pipe "cmd1" "cmd2")` | string | Unix pipes |

## Benefits Summary

### Native (Tier 1)
- ✅ **Type Safety** - Records, not strings
- ✅ **Performance** - No process spawning
- ✅ **Cross-platform** - Works everywhere
- ✅ **Composable** - Natural Scheme integration
- ❌ **Limited scope** - Only common operations

### Tables (Tier 2)
- ✅ **Structured** - Consistent table interface
- ✅ **Operations** - Filter, sort, join built-in
- ✅ **Export** - CSV, Markdown, HTML
- ✅ **Consistency** - Same API for all table commands
- ❌ **Overhead** - Parsing into table format

### Shell (Tier 3)
- ✅ **Flexible** - Any command works
- ✅ **Familiar** - Standard Unix tools
- ✅ **Powerful** - Full shell features
- ❌ **String output** - Manual parsing needed
- ❌ **Platform-dependent** - May not work everywhere

## Use Case Guide

### Data Analysis
```scheme
;; Tier 1: Load data
(define raw-data (read-file "sales.csv"))

;; Tier 1: Process
(define cleaned (parse-csv raw-data))

;; Tier 2: Analyze (if using external tool)
(define summary (csv-stats "sales.csv"))
(display-table summary)

;; Tier 3: Visualize (external tool)
(shell "gnuplot plot.gp")
```

### DevOps Automation
```scheme
;; Tier 1: Check files
(unless (file-exists? "Cargo.toml")
  (error "Not a Rust project"))

;; Tier 3: Build
(shell-check "cargo build --release")

;; Tier 2: Monitor processes
(define rust-procs
  (table-filter (ps)
    (lambda (row)
      (string-contains? (list-ref row 4) "rust"))))

;; Tier 3: Deploy
(shell-check "./deploy.sh")
```

### System Monitoring
```scheme
;; Tier 2: Check disk
(define usage (df))
(define full-disks
  (table-filter usage
    (lambda (row)
      (> (parse-percentage (list-ref row 4)) 90))))

;; Tier 2: High CPU processes
(define hogs
  (table-sort
    (table-filter (ps) (lambda (r) (> (list-ref r 2) 80)))
    'cpu
    #:reverse #t))

;; Alert if problems
(when (> (length full-disks) 0)
  (send-alert "Disk space critical!"))
```

## Migration Path

### From Jupyter
```python
# Jupyter
!ls *.py           →  (ls #:pattern "*.scm")
%cd /tmp           →  (cd "/tmp")
files = !ls        →  (define files (map file-info-name (ls)))
!git log | head    →  (git-log #:max-count 10)
```

### From Unix Shell
```bash
# Shell
ls -la             →  (ls #:long #t #:all #t)
find . -name "*.c" →  (find "." #:name "*.c")
ps aux | grep rust →  (table-filter (ps) pred)
df -h              →  (df)  ; Already human-readable table!
```

## Implementation Priority

1. **Phase 1** (Essential)
   - `(ls)`, `(file-info)`, `(read-file)`, `(write-file)`
   - `(shell)`, `(shell-capture)`

2. **Phase 2** (Table API)
   - `table` record type
   - `(ps)`, `(df)`, `(git-log)`
   - `table-filter`, `table-sort`

3. **Phase 3** (Advanced)
   - `(find)`, `(tree)`
   - More table commands
   - Export functions

## Conclusion

The three-tier design provides:
- **Best of both worlds** - Native speed + Shell flexibility
- **Progressive enhancement** - Use native when available
- **Scheme-first** - Everything composes naturally
- **Better than Jupyter** - No magic syntax, pure functions

Choose your tier wisely! 🎯
