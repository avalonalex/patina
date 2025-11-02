# Patina Notebook Format Specification

An S-expression-based notebook format that's both human-readable and Scheme-native.

## Philosophy

Unlike Jupyter's JSON format, Patina notebooks **are valid Scheme programs**. This means:
- You can load them directly in any R7RS Scheme
- They're homoiconic - notebooks are data and code simultaneously
- Version control friendly (text-based, meaningful diffs)
- No impedance mismatch between notebook and language

## File Format: `.scm.nb` (Scheme Notebook)

A Patina notebook is just a Scheme file with special forms that create a notebook structure:

```scheme
;; my-analysis.scm.nb
(notebook
  (metadata
    (title "Data Analysis with Patina")
    (author "Your Name")
    (created "2025-10-27T10:00:00Z")
    (version "0.1.0"))

  (cell markdown
    "# Introduction

     This notebook demonstrates data analysis in Scheme.")

  (cell code
    (define data '(1 4 2 8 5 7)))

  (cell code
    (define (average lst)
      (/ (apply + lst) (length lst))))

  (cell code
    (average data))

  (cell markdown
    "The average is **5.0**")

  (cell code
    (plot:histogram data
      #:title "Data Distribution"
      #:bins 5)))
```

## Why S-expressions?

### 1. Homoiconicity
The notebook **is** a Scheme program:

```scheme
;; Load a notebook as a library
(import (my-analysis))

;; Access definitions from the notebook
(average my-data)  ; Uses average function from notebook
```

### 2. Composability
Notebooks can include other notebooks:

```scheme
(notebook
  (import-notebook "common-definitions.scm.nb")

  (cell code
    (use-function-from-other-notebook)))
```

### 3. Macros Work Naturally
Define macros that operate on cells:

```scheme
(define-syntax benchmark-cell
  (syntax-rules ()
    ((benchmark-cell expr)
     (cell code
       (time (begin expr))))))

(benchmark-cell
  (factorial 1000))
```

## Complete Specification

### Notebook Structure

```scheme
(notebook
  [metadata]
  [cell*])
```

### Metadata

```scheme
(metadata
  (title STRING)
  (author STRING)
  (created ISO8601-DATE)
  (modified ISO8601-DATE)
  (version STRING)
  (keywords STRING-LIST)
  (tags SYMBOL-LIST)
  (dependencies LIB-LIST))
```

### Cell Types

#### Code Cell
```scheme
(cell code
  [#:id SYMBOL]
  [#:dependencies (CELL-ID ...)]
  [#:output OUTPUT-SPEC]
  EXPRESSION ...)

;; Example
(cell code
  #:id compute-result
  #:dependencies (load-data preprocess)
  (map square data))
```

#### Markdown Cell
```scheme
(cell markdown
  [#:id SYMBOL]
  STRING)

;; Example
(cell markdown
  #:id introduction
  "# My Analysis

   This uses **Scheme**!")
```

#### Output Cell (auto-generated)
```scheme
(cell output
  #:from CELL-ID
  #:timestamp ISO8601-DATE
  #:execution-time MILLISECONDS
  VALUE)
```

#### Rich Output Cell
```scheme
(cell output
  #:from CELL-ID
  #:type (image/png)
  #:data BASE64-STRING)
```

## On-Disk Format

### Pure S-expression (Recommended)

```scheme
;; analysis.scm.nb - Pure Scheme, can be loaded directly!
(notebook
  (metadata (title "Analysis"))

  (cell code
    (define x 42))

  ;; Outputs are stored separately or embedded
  (cell output
    #:from cell-1
    #:timestamp "2025-10-27T10:30:00Z"
    #:execution-time 5
    42))
```

**Advantages:**
- Valid Scheme file
- Can `(load "analysis.scm.nb")`
- Git-friendly diffs
- Comments work naturally
- Syntax highlighting in any Scheme editor

### Separate Output Storage (Alternative)

Keep code and output separate:

**analysis.scm.nb** (code only):
```scheme
(notebook
  (metadata (title "Analysis"))

  (cell code #:id cell-1
    (define x 42))

  (cell code #:id cell-2
    (+ x 8)))
```

**analysis.scm.nb.outputs** (execution results):
```scheme
(outputs
  (cell-1
    (timestamp "2025-10-27T10:30:00Z")
    (execution-time 5)
    (value 42))

  (cell-2
    (timestamp "2025-10-27T10:30:15Z")
    (execution-time 3)
    (value 50)))
```

**Advantages:**
- Clean separation
- Can `.gitignore` outputs
- Notebook file is pure code
- Re-run from scratch easily

## System Commands: The Elegant Way

Instead of Jupyter's clunky `!command` or `%magic`, use **proper Scheme procedures**:

### Design: Everything is a Function

```scheme
;; Bad (Jupyter style)
!ls -la
%system git status

;; Good (Patina style)
(shell "ls -la")
(shell "git status")

;; Even better - proper procedures
(directory-list ".")
(git 'status)
```

### Built-in System Integration

```scheme
(library (patina system)
  (export
    shell           ; Execute shell command
    sh              ; Alias for shell
    directory-list  ; List directory
    file-read       ; Read file
    file-write      ; Write file
    git             ; Git integration
    env             ; Environment variables
    pipe            ; Unix pipes
    process         ; Process management
    ))

;; Usage in notebook
(cell code
  (import (patina system))

  ;; Execute shell command
  (shell "cargo build --release"))

;; Get structured output
(cell code
  (define files (directory-list "." #:pattern "*.rs"))
  (length files))

;; Git integration
(cell code
  (git 'status)
  (git 'log #:max-count 5)
  (git 'diff "HEAD~1"))

;; Pipes and composition
(cell code
  (pipe
    (shell "cat data.csv")
    (grep "pattern")
    (wc '-l)))
```

### Advanced: Make Your Own Commands

Since notebooks are Scheme, define domain-specific operations:

```scheme
(cell code
  ;; Define a custom analysis command
  (define-syntax data-pipeline
    (syntax-rules (load transform analyze visualize)
      ((data-pipeline
         (load SOURCE)
         (transform EXPR ...)
         (analyze STAT)
         (visualize TYPE))
       (let* ((data (read-data SOURCE))
              (cleaned (begin EXPR ...))
              (result (STAT cleaned)))
         (plot TYPE result))))))

;; Use it
(cell code
  (data-pipeline
    (load "data.csv")
    (transform
      (filter positive? data)
      (map normalize data))
    (analyze mean)
    (visualize histogram)))
```

### Shell Command DSL

Create a nice DSL for shell interaction:

```scheme
(cell code
  (define-syntax $
    (syntax-rules ()
      (($ cmd args ...)
       (shell (string-append cmd " " args ...)))))

  ;; Now you can write
  ($ "git" "status")
  ($ "ls" "-la")

  ;; With interpolation
  (let ((branch "main"))
    ($ "git" "checkout" branch)))
```

## Literate Programming Features

### Tangling (Extract Code)

```scheme
;; In notebook
(cell code #:tangle "output.scm"
  (define (my-function x)
    (* x 2)))

;; Command to extract
(tangle-notebook "analysis.scm.nb" #:output "lib/")
```

### Weaving (Generate Documentation)

```scheme
;; Generate HTML from notebook
(weave-notebook "analysis.scm.nb"
  #:format 'html
  #:output "docs/analysis.html"
  #:theme 'github)

;; Generate markdown
(weave-notebook "analysis.scm.nb"
  #:format 'markdown
  #:output "README.md")
```

## Cell Dependencies and Reactivity

```scheme
(notebook
  ;; Cell A
  (cell code #:id load-data
    (define data (read-csv "data.csv")))

  ;; Cell B depends on A
  (cell code #:id process-data
    #:depends-on (load-data)
    (define cleaned (remove-outliers data)))

  ;; Cell C depends on B
  (cell code #:id visualize
    #:depends-on (process-data)
    (plot:scatter cleaned)))

;; When load-data is re-evaluated,
;; process-data and visualize are marked "stale"
```

## Notebook Operations

### As a Library

```scheme
(library (patina notebook)
  (export
    notebook?
    notebook-cells
    notebook-metadata
    cell-execute
    cell-output
    cell-dependencies
    mark-stale
    rerun-stale
    save-notebook
    load-notebook
    export-notebook))
```

### Usage

```scheme
;; Load notebook
(define nb (load-notebook "analysis.scm.nb"))

;; Inspect
(notebook-metadata nb)
(notebook-cells nb)

;; Execute cells
(cell-execute nb 'cell-5)

;; Check dependencies
(cell-dependencies nb 'cell-5)
;; => (cell-2 cell-3)

;; Mark dirty and rerun
(mark-stale nb 'cell-2)
(rerun-stale nb)

;; Export
(export-notebook nb
  #:format 'html
  #:output "report.html")
```

## Comparison with Jupyter

| Feature | Jupyter (.ipynb) | Patina (.scm.nb) |
|---------|------------------|------------------|
| Format | JSON | S-expressions |
| Loadable as code | ❌ No | ✅ Yes |
| Git-friendly | ⚠️ Difficult | ✅ Easy |
| System commands | `!cmd` magic | `(shell "cmd")` |
| Extensible | Python-specific | Scheme macros |
| Language agnostic | ✅ Yes | ❌ Scheme-only |
| Cell types | Hard-coded | Macro-extensible |
| Literate programming | Limited | Full support |

## Example: Full Notebook

```scheme
;; data-analysis.scm.nb
(notebook
  (metadata
    (title "Sales Analysis Q4 2024")
    (author "Data Team")
    (created "2025-10-27")
    (dependencies (patina system) (patina plot)))

  (import (patina system)
          (patina plot))

  (cell markdown
    "# Sales Analysis Q4 2024

     This notebook analyzes our quarterly sales data.")

  (cell code #:id load-data
    "Load the raw sales data"
    (define raw-data
      (shell "curl -s https://api.example.com/sales/q4")))

  (cell code #:id parse-data
    #:depends-on (load-data)
    (define sales
      (json->scheme raw-data)))

  (cell code #:id summary-stats
    #:depends-on (parse-data)
    (define total-sales (apply + (map cdr sales)))
    (define avg-sale (/ total-sales (length sales)))
    (display-table
      `((Metric Value)
        (Total ,total-sales)
        (Average ,avg-sale)
        (Count ,(length sales)))))

  (cell markdown
    (format "Total sales: ~a" total-sales))

  (cell code #:id visualization
    #:depends-on (parse-data)
    (plot:line sales
      #:title "Sales Trend"
      #:x-label "Date"
      #:y-label "Revenue"
      #:save "sales-trend.png"))

  (cell code #:id export-report
    "Export results for stakeholders"
    (let ((report (generate-report sales)))
      (file-write "report.csv" report)
      (git 'add "report.csv")
      (git 'commit "-m" "Update Q4 report")))

  (cell markdown
    "## Conclusion

     Analysis complete. Report committed to git."))
```

## Implementation Notes

### Parser

Simple recursive descent parser for notebook format:

```scheme
(define (parse-notebook input)
  (match input
    (('notebook ('metadata . meta-pairs) . cells)
     (make-notebook
       (parse-metadata meta-pairs)
       (map parse-cell cells)))
    (_ (error "Invalid notebook format"))))
```

### Cell Execution

Track execution state:

```scheme
(define-record-type cell
  (fields id type code output dependencies stale? execution-time))

(define (execute-cell! notebook cell-id)
  (let ((cell (notebook-find-cell notebook cell-id)))
    (when (cell-stale? cell)
      ;; Execute dependencies first
      (for-each (lambda (dep)
                  (execute-cell! notebook dep))
                (cell-dependencies cell))

      ;; Execute this cell
      (let-values (((result time) (time-eval (cell-code cell))))
        (cell-output-set! cell result)
        (cell-execution-time-set! cell time)
        (cell-stale-set! cell #f)))))
```

## Future Extensions

### 1. Remote Execution
```scheme
(cell code #:execute-on "cluster.example.com"
  (big-data-processing dataset))
```

### 2. Parallel Cells
```scheme
(cell code #:parallel #t
  (parallel-map expensive-computation data))
```

### 3. Conditional Execution
```scheme
(cell code #:when (file-exists? "cache.scm")
  (load "cache.scm"))
```

### 4. Cell Templates
```scheme
(define-cell-template data-loader
  (lambda (source)
    (cell code
      (define data (read-csv ,source))
      (display-summary data))))

(data-loader "sales.csv")
(data-loader "inventory.csv")
```

## Conclusion

S-expression notebooks provide:
- **Simplicity** - Just Scheme!
- **Power** - Full language at your disposal
- **Elegance** - No magic syntax, everything is a function
- **Extensibility** - Define your own cell types and commands
- **Integration** - Notebooks are libraries

Instead of bolting features onto a notebook format, we extend the language itself.
