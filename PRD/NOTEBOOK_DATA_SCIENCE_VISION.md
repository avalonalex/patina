# Patina Notebooks + Data Science: A Killer Combination

**Last Updated:** 2025-11-09
**Status:** Strategic Vision

---

## The Big Idea

Combine **Patina's S-expression notebooks** with **Rust-powered data science tools** to create the **best data science notebook for functional programming**.

**Unique positioning:**
- Jupyter is great but Python-centric and JSON-based
- Observable is great but JavaScript-centric and cloud-based
- Org-mode is great but Emacs-centric and limited data tools
- **Patina notebooks:** Scheme-native, terminal-based, Rust-powered data science

---

## Why This Combination is Powerful

### 1. S-Expression Notebooks Are Perfect for Data Science

**Notebooks as valid Scheme programs:**
```scheme
;; analysis.scm.nb - This is a valid Scheme file!
(notebook
  (metadata (title "Sales Analysis"))

  (cell code
    (define sales (dataframe-from-csv "sales.csv")))

  (cell code
    (dataframe-group-by sales 'region
      (lambda (group) (mean (get group 'revenue))))))

;; You can load this as a library!
(import (analysis))
(use sales)  ; Access the sales dataframe from anywhere
```

**Git-friendly:**
- Text-based format, not JSON
- Meaningful diffs (see exactly what code changed)
- No merge conflicts from cell IDs or output data
- Can separate outputs from code (`.scm.nb` + `.scm.nb.outputs`)

**Reproducible:**
- Dependency tracking between cells
- Deterministic execution order
- Can re-run from scratch easily
- Notebooks are documentation + code + tests in one

---

### 2. Rust-Powered Data Tools are Fast

**Zero-copy FFI with Polars/ndarray:**
```scheme
;; Create dataframe (wraps Rust Polars directly)
(define df (dataframe '((name . ["Alice" "Bob" "Carol"])
                        (age . [25 30 35])
                        (salary . [50000 60000 70000]))))

;; Query with Polars speed (zero-copy!)
(-> df
    (filter (lambda (row) (> (get row 'age) 28)))
    (select '(name salary))
    (sort-by 'salary 'desc))
;; => Fast! Uses Polars internally
```

**Performance comparison:**
- **Pandas:** Pure Python, slow for large datasets
- **Polars:** Rust-native, 5-10x faster than Pandas
- **Patina + Polars:** Zero-copy FFI = Polars speed from Scheme!

**Leverage Rust ecosystem:**
- **Polars** - DataFrames (like Pandas but faster)
- **ndarray** - Numeric arrays (like NumPy)
- **plotters** - High-performance plotting
- **arrow** - Columnar data format
- **rayon** - Parallel iterators

---

### 3. Terminal UI is Ideal for Data Exploration

**Why terminal > web browser:**

**Speed:**
- No browser startup overhead
- Instant cell execution visualization
- No HTTP round-trips
- No JavaScript runtime

**Integration:**
- Works over SSH (remote servers)
- Integrates with tmux/screen (persistent sessions)
- Shell commands via `(shell "...")`
- Git integration built-in

**Workflow:**
```scheme
;; In notebook, seamlessly mix Scheme and shell
(cell code
  (define files (shell "find data/ -name '*.csv'")))

(cell code
  (for-each process-file (string-split files "\n")))

;; Git integration
(cell code
  (git 'add "results.csv")
  (git 'commit "-m" "Update analysis"))
```

---

## Proposed Feature Set

### Phase 1: Core Notebook + Dataframes (4-6 weeks)

**Notebook Infrastructure:**
- Terminal UI with Ratatui (vim-style keybindings)
- Cell-based editing (code cells + markdown cells)
- Dependency tracking (reactive cells)
- Save/load `.scm.nb` files
- Execute cells, show outputs

**Dataframe Library (Polars wrapper):**
```scheme
(import (patina dataframe))

;; Create
(dataframe-from-csv "data.csv")
(dataframe '((col1 . [1 2 3]) (col2 . ["a" "b" "c"])))

;; Query
(dataframe-filter df predicate)
(dataframe-select df '(col1 col2))
(dataframe-group-by df 'col aggregation-fn)
(dataframe-sort-by df 'col 'asc)
(dataframe-join df1 df2 'key)

;; Lazy evaluation (Polars LazyFrame)
(-> df
    (select '(name age))
    (filter (lambda (r) (> (get r 'age) 25)))
    (lazy))

(collect lazy-df)  ; Execute query plan

;; Output
(dataframe-to-csv df "output.csv")
(dataframe->list df)
(dataframe->alist df)
```

**Display in Notebook:**
```scheme
(cell code
  (define df (dataframe-from-csv "sales.csv")))

;; Output shows:
┌───────────────────────────────────────┐
│ Dataframe: sales.csv (1000 rows)     │
├─────────┬─────┬──────────┬───────────┤
│ name    │ age │ region   │ revenue   │
├─────────┼─────┼──────────┼───────────┤
│ Alice   │ 25  │ West     │ 50000     │
│ Bob     │ 30  │ East     │ 60000     │
│ Carol   │ 35  │ West     │ 70000     │
│ ...     │ ... │ ...      │ ...       │
└─────────┴─────┴──────────┴───────────┘
[Showing first 3 of 1000 rows]
```

**Estimated effort:** 4-6 weeks
- 2-3 weeks: Notebook infrastructure (TUI, cells, execution)
- 2-3 weeks: Polars FFI wrapper (basic operations)

---

### Phase 2: Visualization (2-3 weeks)

**Plotting Library:**
```scheme
(import (patina plot))

;; Line plot
(plot-line data
  #:x-col 'date
  #:y-col 'revenue
  #:title "Revenue Over Time"
  #:save "plot.png")

;; Scatter plot
(plot-scatter df
  #:x 'age
  #:y 'salary
  #:color-by 'region)

;; Histogram
(plot-histogram (dataframe-col df 'age)
  #:bins 20
  #:title "Age Distribution")

;; Bar chart
(plot-bar summary-df
  #:x 'category
  #:y 'count)

;; Heatmap
(plot-heatmap correlation-matrix)
```

**Terminal Graphics:**

**Option 1: Sixel/Kitty graphics protocol**
- Display images directly in terminal
- Works in iTerm2, Kitty, WezTerm
- High quality

**Option 2: ASCII/Unicode plots**
- Works everywhere
- Lower quality but universal

**Option 3: Save to file + open**
```scheme
(plot-line data #:save "plot.png")
(shell "open plot.png")  ; Or display inline if terminal supports
```

**Implementation:**
- Use Rust `plotters` crate (high-performance plotting)
- Generate images, display in terminal or save to file
- Interactive zooming/panning in TUI (optional)

**Estimated effort:** 2-3 weeks

---

### Phase 3: Numeric Arrays (2-3 weeks)

**Array Library (ndarray wrapper):**
```scheme
(import (patina array))

;; Create arrays
(array [1 2 3 4 5])
(array [[1 2] [3 4]])
(zeros [3 3])
(ones [2 4])
(linspace 0 10 100)

;; Operations (SIMD optimized via ndarray)
(array-add a b)
(array-mul a b)
(array-dot matrix1 matrix2)
(array-transpose matrix)

;; Broadcasting
(array-add matrix scalar)  ; Add scalar to all elements

;; Slicing
(array-slice a 1 5)
(array-slice-2d matrix [0 2] [1 3])  ; Rows 0-2, cols 1-3

;; Reduction
(array-sum a)
(array-mean a)
(array-std a)

;; Element-wise functions
(array-map sqrt a)
(array-map-2 + a b)
```

**Integration with dataframes:**
```scheme
;; Convert between dataframes and arrays
(dataframe-col-as-array df 'age)  ; Extract column as array
(array-to-dataframe arr '(col1 col2 col3))  ; Array -> dataframe
```

**Estimated effort:** 2-3 weeks

---

### Phase 4: Machine Learning (Optional, 4-6 weeks)

**ML Library (smartcore wrapper):**
```scheme
(import (patina ml))

;; Linear regression
(define model (linear-regression))
(train! model X y)
(predict model X-test)

;; Decision trees
(define tree (decision-tree-classifier #:max-depth 5))
(train! tree X y)
(predict tree X-test)

;; K-means clustering
(define kmeans (kmeans #:n-clusters 3))
(fit! kmeans X)
(predict kmeans X-test)

;; Model evaluation
(accuracy y-true y-pred)
(confusion-matrix y-true y-pred)
(roc-auc y-true y-scores)
```

**Estimated effort:** 4-6 weeks (if there's demand)

---

## Example: Complete Data Science Workflow

```scheme
;; sales-analysis.scm.nb
(notebook
  (metadata
    (title "Q4 2024 Sales Analysis")
    (author "Data Team")
    (created "2025-11-09"))

  (import (patina dataframe)
          (patina plot)
          (patina array)
          (patina system))

  ;;─────────────────────────────────────
  ;; 1. Load Data
  ;;─────────────────────────────────────

  (cell markdown
    "# Q4 2024 Sales Analysis

     This notebook analyzes quarterly sales performance across regions.")

  (cell code #:id load-data
    "Load sales data from CSV"
    (define sales (dataframe-from-csv "data/sales-q4.csv"))
    (display-dataframe sales #:limit 5))

  (cell code
    "Data shape and summary"
    (format "~a rows, ~a columns"
            (dataframe-nrows sales)
            (dataframe-ncols sales))
    (dataframe-describe sales))

  ;;─────────────────────────────────────
  ;; 2. Data Cleaning
  ;;─────────────────────────────────────

  (cell markdown
    "## Data Cleaning

     Remove outliers and handle missing values.")

  (cell code #:id clean-data
    #:depends-on (load-data)
    (define clean-sales
      (-> sales
          (dataframe-drop-na 'revenue)
          (dataframe-filter
            (lambda (row)
              (and (> (get row 'revenue) 0)
                   (< (get row 'revenue) 1000000)))))))

  (cell code
    (format "Removed ~a outliers"
            (- (dataframe-nrows sales)
               (dataframe-nrows clean-sales))))

  ;;─────────────────────────────────────
  ;; 3. Regional Analysis
  ;;─────────────────────────────────────

  (cell markdown
    "## Regional Performance")

  (cell code #:id regional-summary
    #:depends-on (clean-data)
    (define by-region
      (dataframe-group-by clean-sales 'region
        (lambda (group)
          (list (mean (dataframe-col group 'revenue))
                (sum (dataframe-col group 'revenue))
                (length (dataframe-col group 'revenue))))))

    (dataframe-sort-by by-region 'total 'desc))

  (cell code
    "Visualize regional revenue"
    (plot-bar by-region
      #:x 'region
      #:y 'total
      #:title "Total Revenue by Region"
      #:save "plots/revenue-by-region.png"))

  ;;─────────────────────────────────────
  ;; 4. Time Series Analysis
  ;;─────────────────────────────────────

  (cell markdown
    "## Revenue Trend Over Time")

  (cell code #:id time-series
    #:depends-on (clean-data)
    (define daily-revenue
      (-> clean-sales
          (dataframe-group-by 'date
            (lambda (g) (sum (dataframe-col g 'revenue))))
          (dataframe-sort-by 'date 'asc))))

  (cell code
    (plot-line daily-revenue
      #:x 'date
      #:y 'revenue
      #:title "Daily Revenue Trend"
      #:save "plots/daily-trend.png"))

  ;;─────────────────────────────────────
  ;; 5. Correlation Analysis
  ;;─────────────────────────────────────

  (cell markdown
    "## Feature Correlation")

  (cell code
    "Compute correlation matrix"
    (define numeric-cols
      (dataframe-select clean-sales
        '(revenue quantity discount customer-age)))

    (define corr-matrix
      (array-correlation
        (dataframe->array numeric-cols))))

  (cell code
    (plot-heatmap corr-matrix
      #:labels '(revenue quantity discount customer-age)
      #:title "Feature Correlation"
      #:save "plots/correlation.png"))

  ;;─────────────────────────────────────
  ;; 6. Statistical Testing
  ;;─────────────────────────────────────

  (cell markdown
    "## Regional Revenue Comparison

     Test if regional means are significantly different.")

  (cell code
    (define west-revenue
      (dataframe-col
        (dataframe-filter clean-sales
          (lambda (r) (equal? (get r 'region) "West")))
        'revenue))

    (define east-revenue
      (dataframe-col
        (dataframe-filter clean-sales
          (lambda (r) (equal? (get r 'region) "East")))
        'revenue))

    ;; T-test
    (define t-result (t-test west-revenue east-revenue))
    (format "p-value: ~a" (get t-result 'p-value)))

  ;;─────────────────────────────────────
  ;; 7. Export Results
  ;;─────────────────────────────────────

  (cell markdown
    "## Export Results")

  (cell code #:id export-results
    #:depends-on (regional-summary time-series)

    ;; Save processed data
    (dataframe-to-csv by-region "output/regional-summary.csv")
    (dataframe-to-csv daily-revenue "output/daily-revenue.csv")

    ;; Commit to git
    (git 'add "output/" "plots/")
    (git 'commit "-m" "Update Q4 analysis results")

    "✓ Results exported and committed"))
```

**Output in terminal:**
```
╔═══════════════════════════════════════════════════════════════╗
║ Patina Notebook - sales-analysis.scm.nb          [Modified]  ║
╠═══════════════════════════════════════════════════════════════╣
║                                                                ║
║ ┌─ [1] Markdown ───────────────────────────────────────────┐  ║
║ │ # Q4 2024 Sales Analysis                                │  ║
║ │                                                          │  ║
║ │ This notebook analyzes quarterly sales performance      │  ║
║ │ across regions.                                          │  ║
║ └──────────────────────────────────────────────────────────┘  ║
║                                                                ║
║ ┌─ [2] Code ──────────────────────────────────────┐ [0.2s] ✓  ║
║ │ (define sales (dataframe-from-csv ...))         │           ║
║ └─────────────────────────────────────────────────┘           ║
║ Output:                                                        ║
║ ┌────────────────────────────────────────────────────────┐    ║
║ │ Dataframe: sales-q4.csv (5234 rows, 8 columns)        │    ║
║ ├──────┬──────┬─────────┬──────────┬─────────┬──────────┤    ║
║ │ date │ region│ product │ revenue  │ quantity│ discount │    ║
║ ├──────┼───────┼─────────┼──────────┼─────────┼──────────┤    ║
║ │ 10/1 │ West  │ Widget  │ 12500.00 │ 25      │ 0.10     │    ║
║ │ 10/1 │ East  │ Gadget  │ 8900.00  │ 12      │ 0.05     │    ║
║ │ ...  │ ...   │ ...     │ ...      │ ...     │ ...      │    ║
║ └──────┴───────┴─────────┴──────────┴─────────┴──────────┘    ║
║ [Showing 5 of 5234 rows]                                      ║
...
```

---

## Competitive Analysis

| Feature | Jupyter | Observable | Org-mode | **Patina** |
|---------|---------|------------|----------|------------|
| **Format** | JSON | JavaScript | Org | **S-expressions** |
| **Loadable as code** | ❌ | ❌ | ⚠️ | **✅ Yes** |
| **Git-friendly** | ⚠️ | ⚠️ | ✅ | **✅ Yes** |
| **Terminal-based** | ❌ | ❌ | ⚠️ | **✅ Yes** |
| **Data performance** | ⚠️ Pandas | ⚠️ JS | ❌ | **✅ Polars (Rust)** |
| **Functional** | ⚠️ | ⚠️ | ⚠️ | **✅ Scheme** |
| **Type safety** | ❌ | ⚠️ TS | ❌ | **✅ Gradual typing** |
| **Shell integration** | `!cmd` | ❌ | ✅ | **✅ `(shell "...")`** |
| **Reproducible** | ⚠️ | ⚠️ | ✅ | **✅ Dependency tracking** |

---

## Unique Selling Points

### 1. **Only Scheme Notebook with Production Data Tools**
- No other Scheme has dataframes/arrays
- Leverage Rust ecosystem (Polars, ndarray)
- Functional programming + data science

### 2. **Terminal-First, SSH-Friendly**
- No browser required
- Works over SSH (remote servers)
- Integrates with terminal workflow
- Persistent with tmux/screen

### 3. **Notebooks are Programs**
- `.scm.nb` files are valid Scheme
- Can import notebooks as libraries
- Macros work naturally
- No JSON impedance mismatch

### 4. **Gradual Typing for Data Pipelines**
```scheme
;; Add types to catch errors early
(: process-data (-> Dataframe Dataframe))
(define (process-data df)
  (-> df
      (filter valid-row?)
      (transform normalize-revenue)))

;; Type errors caught at "compile time"
(process-data 42)  ; Error: expected Dataframe, got Integer
```

### 5. **Reproducible by Default**
- Dependency tracking between cells
- Deterministic execution
- Git-friendly format
- Separate outputs from code

---

## Target Audiences

### Data Scientists
**Pain point:** Jupyter is messy, hard to version control
**Patina solution:** Git-friendly notebooks + fast data tools

### Functional Programmers
**Pain point:** No good data science tools for functional languages
**Patina solution:** Scheme + Polars/ndarray

### Terminal Enthusiasts
**Pain point:** Browser-based notebooks break terminal workflow
**Patina solution:** Terminal-native, vim keybindings

### Researchers
**Pain point:** Reproducibility is hard
**Patina solution:** Notebooks as programs + dependency tracking

---

## Implementation Roadmap

### Phase 1: MVP (6-8 weeks)
**Goal:** Basic notebook + dataframes

1. **Notebook Infrastructure** (3-4 weeks)
   - Terminal UI with Ratatui
   - Cell editing (code + markdown)
   - Execute cells, show outputs
   - Save/load `.scm.nb` files

2. **Dataframe Library** (3-4 weeks)
   - Rust FFI to Polars
   - Basic operations (filter, select, group-by, join)
   - Display dataframes in TUI
   - CSV import/export

**Deliverable:** Can do basic data analysis in terminal notebook

---

### Phase 2: Visualization (2-3 weeks)
**Goal:** Plot data in notebooks

1. **Plotting Library**
   - Rust FFI to plotters
   - Line, scatter, bar, histogram
   - Terminal graphics (sixel/kitty) or save to file

**Deliverable:** Can create visualizations in notebooks

---

### Phase 3: Numeric Arrays (2-3 weeks)
**Goal:** NumPy-like arrays

1. **Array Library**
   - Rust FFI to ndarray
   - Array operations (add, mul, dot, transpose)
   - SIMD optimization via ndarray
   - Integration with dataframes

**Deliverable:** Can do numeric computing

---

### Phase 4: Polish (2-3 weeks)
**Goal:** Production-ready

1. **UX Improvements**
   - Better error messages
   - Syntax highlighting
   - Autocomplete
   - History search

2. **Performance**
   - Lazy evaluation for large dataframes
   - Streaming CSV reading
   - Parallel cell execution

3. **Documentation**
   - Tutorial notebooks
   - API documentation
   - Example gallery

**Deliverable:** Production-ready data science notebook

---

### Total Estimated Time: 12-17 weeks (3-4 months)

---

## Marketing Strategy

### Tagline
> "Functional Data Science in Your Terminal"

### Messaging

**For Data Scientists:**
"Tired of Jupyter's JSON format and slow Pandas? Try Patina: terminal-based notebooks with Rust-powered dataframes that are 5-10x faster."

**For Scheme Developers:**
"Finally, a Scheme for data science! Notebooks that are valid Scheme programs + production-grade data tools."

**For Terminal Users:**
"Data science without leaving your terminal. Vim keybindings, git-friendly, SSH-compatible."

**For Researchers:**
"Reproducible research made easy. Notebooks are programs, dependencies are tracked, everything is version controlled."

---

## Success Metrics

**Phase 1 Success:**
- 100+ notebook users
- 5+ example notebooks in gallery
- Basic dataframe operations working
- Positive feedback on terminal UX

**Phase 2 Success:**
- 500+ notebook users
- Used in at least one research paper
- Performance benchmarks show Polars speed
- LSP integration for notebooks

**Phase 3 Success:**
- 1000+ users
- Community-contributed notebooks
- Featured in data science communities
- "Best terminal notebook" reputation

---

## Conclusion

**Patina Notebooks + Data Science = Unique Opportunity**

**No one else is doing this:**
- Scheme notebooks with production data tools
- Terminal-first data science
- Git-friendly, reproducible by default
- Fast (Rust/Polars backend)

**Estimated effort:** 3-4 months to MVP
**Payoff:** Unique position in data science + Scheme ecosystems

**Next steps:**
1. Complete Phase 1 (R7RS compliance)
2. Implement Rust FFI (enables data tools)
3. Build notebook MVP (3-4 months)
4. Launch with tutorial and example gallery

This could be **the** feature that makes Patina famous in both Scheme and data science communities.
