# Patina: Unique Features Analysis

**Purpose:** Identify features that would make Patina a unique and valuable Scheme implementation
**Last Updated:** 2025-11-09

---

## Current Differentiators (What Makes Us Unique Now)

### 1. Educational Focus with Production Quality ✅
**Status:** Already a core goal

- **Understandable implementation** - Clean, well-documented Rust code
- **Comprehensive testing** - 395 tests with detailed compliance tracking
- **Rich debugging facilities** - Debug mode, upcoming hook system
- **Learning resource** - Can learn both Scheme and interpreter design

**Unique value:** Most Schemes optimize for *either* education (simple but incomplete) *or* production (fast but complex). Patina aims for both.

---

### 2. Rust-Native Implementation ✅
**Status:** Current architecture

- Written in idiomatic Rust
- Uses Rust's type system for safety
- Leverages Rust ecosystem (num-bigint, num-rational, rustyline)
- Memory safe without GC overhead (Rc/RefCell model)

**Unique value:** Only mature Scheme in Rust besides Steel

---

## Proposed Unique Features

### Category A: Embedding & FFI (HIGH Priority)

#### 1. **Rust FFI with Zero-Copy Semantics** ⭐⭐⭐⭐⭐

**The Big Idea:** Make Patina the **best embeddable Scheme for Rust projects**

**Key features:**
```rust
// From Rust: Call Scheme with zero-copy
let engine = Interpreter::new();
let result = engine.call("process-data", &my_rust_struct)?;

// From Scheme: Call Rust functions naturally
(rust-fn "my_crate::process" data)

// Share data structures without serialization
(define rust-vec (rust-vec-new))
(rust-vec-push! rust-vec 42)  ; Mutates Rust Vec directly
```

**Why unique:**
- **Steel's approach:** Good but complex contract system
- **Guile's approach:** C FFI, not Rust-native
- **Patina's approach:** Leverage Rust's type system for safety + performance

**Implementation strategy:**

**Phase 1: Basic FFI (2-3 weeks)**
```rust
// Register Rust functions
engine.register_fn("add", |a: i64, b: i64| a + b);
engine.register_fn("process", process_data);

// Automatic conversion for basic types
impl FromScheme for MyStruct { ... }
impl ToScheme for MyResult { ... }
```

**Phase 2: Zero-Copy (2-3 weeks)**
```rust
// Wrap Rust types directly in Scheme values
pub enum Value {
    // ... existing variants ...
    RustValue(Box<dyn Any>),  // Opaque Rust value
    RustRef(&'static dyn Any), // Borrowed reference
}

// Access from Scheme
(rust-call obj "method_name" args...)
(rust-get obj "field_name")
(rust-set! obj "field_name" value)
```

**Phase 3: Advanced (3-4 weeks)**
- Async Rust function support
- Trait objects as Scheme objects
- Derive macros for automatic FFI generation
- Rust iterators as Scheme streams

**Use cases:**
- **Game scripting:** Rust game engine + Scheme scripting
- **Config/DSL:** Rust app + Scheme config language
- **Data processing:** Rust performance + Scheme flexibility
- **Plugin systems:** Load Scheme plugins into Rust apps

**Estimated effort:** 2-3 months for full system
**Market gap:** No Scheme has truly excellent Rust FFI

---

#### 2. **WASM Compilation Target** ⭐⭐⭐⭐

**The Big Idea:** Run Patina Scheme in web browsers and edge computing

```scheme
; Compile Scheme to WASM
(compile-to-wasm 'my-module "output.wasm")

; Run in browser
<script src="patina.wasm"></script>
<script>
  Patina.eval("(+ 1 2)");  // => 3
</script>
```

**Why unique:**
- BiwaScheme exists but limited R7RS support
- Guile has experimental WASM but incomplete
- Racket WASM is nascent

**Implementation approaches:**

**Approach 1: Interpreter in WASM (easier)**
- Compile Rust interpreter to WASM (rustc already supports this!)
- 2-3 weeks to get basic version working
- Larger binary size (~1-2 MB)

**Approach 2: Scheme-to-WASM compiler (harder)**
- Compile Scheme AST to WASM bytecode
- 2-3 months effort
- Smaller, faster output

**Use cases:**
- Interactive documentation (executable code examples)
- Browser-based data analysis
- Edge computing (Cloudflare Workers, Fastly Compute)
- Serverless functions
- Educational tools (online REPL)

**Estimated effort:** 3-4 weeks (Approach 1), 2-3 months (Approach 2)
**Market gap:** Few modern Schemes target WASM well

---

### Category B: Advanced Type Systems (MEDIUM-HIGH Priority)

#### 3. **Gradual Typing (Typed Racket style)** ⭐⭐⭐⭐⭐

**The Big Idea:** Optional type annotations for performance and correctness

**Already planned in README as Phase 2!**

```scheme
; Untyped code (dynamic)
(define (factorial n)
  (if (<= n 1) 1 (* n (factorial (- n 1)))))

; Typed code (static checking + optimization)
(: factorial (-> Integer Integer))
(define (factorial n)
  (if (<= n 1) 1 (* n (factorial (- n 1)))))

; Gradual: mix typed and untyped
(: process-data (-> (Listof String) Integer))
(define (process-data items)
  (length (filter valid? items)))  ; valid? is untyped

; Contract checking at boundaries
(: safe-divide (-> Real Real Real))
(define (safe-divide a b)
  (assert (not (= b 0)) "Division by zero")
  (/ a b))
```

**Why unique:**
- Typed Racket has performance issues (runtime contract overhead)
- Patina advantage: Rust's type system for implementation
- Can compile typed code to faster Rust-like operations

**Implementation strategy:**

**Phase 1: Type Annotations (3-4 weeks)**
```scheme
(: name Type)  ; Type annotation syntax
(define (name x) ...)

; Basic types
Integer, Real, Boolean, String, Symbol
(Listof T), (Vectorof T), (Pairof A B)
(-> T1 T2 ... Result)  ; Function types
```

**Phase 2: Type Checker (4-6 weeks)**
- Hindley-Milner style inference
- Polymorphic types
- Contract generation for untyped boundaries

**Phase 3: Optimization (3-4 weeks)**
- Monomorphization (like Rust)
- Inline numeric operations for typed code
- Remove runtime type checks in typed code

**Benefits:**
- **Performance:** 2-10x speedup for typed code
- **Correctness:** Catch errors at compile time
- **Documentation:** Types as machine-checked docs
- **Gradual adoption:** Add types incrementally

**Estimated effort:** 3-4 months for full system
**Market gap:** No Scheme has efficient gradual typing yet

---

#### 4. **Effect System (Advanced)** ⭐⭐⭐

**The Big Idea:** Track side effects in type system

```scheme
; Pure function (no effects)
(: factorial (Pure (-> Integer Integer)))
(define (factorial n)
  (if (<= n 1) 1 (* n (factorial (- n 1)))))

; I/O effect
(: read-config (IO (-> String Config)))
(define (read-config filename)
  (read (open-input-file filename)))

; Multiple effects
(: process-file (IO Exception (-> String String)))
(define (process-file filename)
  (guard (exn (else "error"))
    (let ((data (read-file filename)))
      (write-file "output.txt" (transform data)))))

; Effect polymorphism
(: map (forall [T U E] (E (-> (-> T U) (Listof T) (Listof U)))))
(define (map f lst)
  (if (null? lst) '()
      (cons (f (car lst)) (map f (cdr lst)))))
```

**Why unique:**
- **No Scheme has this**
- Koka, Effekt, OCaml 5 have effects but not in Scheme
- Enables powerful optimizations and reasoning

**Benefits:**
- Prevent accidental side effects
- Enable parallel execution of pure code
- Better error messages ("this function can't do I/O here")
- Compiler optimizations (reorder pure computations)

**Estimated effort:** 4-6 months (research + implementation)
**Market gap:** Completely novel for Scheme

---

### Category C: Reactive & Concurrent (MEDIUM Priority)

#### 5. **Reactive Streams (Project Reactor style)** ⭐⭐⭐⭐

**The Big Idea:** First-class reactive programming in Scheme

**Already planned in README as Phase 3!**

```scheme
; Create stream
(define numbers (stream-range 0 infinity))

; Transform stream
(define evens
  (stream-filter even? numbers))

(define doubled
  (stream-map (lambda (x) (* x 2)) evens))

; Consume stream
(stream-take 10 doubled)
; => (0 4 8 12 16 20 24 28 32 36)

; Hot streams (multiple subscribers)
(define click-stream (hot-stream))

(stream-subscribe! click-stream
  (lambda (event) (display "Clicked!")))

(stream-subscribe! click-stream
  (lambda (event) (log-event event)))

; Backpressure
(define slow-processor
  (stream-buffer 100 'drop-oldest))

; Async streams
(define file-stream
  (async-stream-from-file "large.txt"))

(stream-for-each process-line file-stream)
```

**Why unique:**
- RxScheme exists but unmaintained
- No modern Scheme has reactive primitives
- Rust has Tokio - can leverage for async backend

**Implementation with Rust integration:**

```rust
// Rust side: async runtime
use tokio::sync::mpsc;

struct SchemeStream {
    receiver: mpsc::Receiver<Value>,
    // ...
}

// Scheme side: reactive operators
(stream-from-rust-channel channel)
(stream-merge stream1 stream2)
(stream-debounce ms stream)
```

**Use cases:**
- Event-driven UIs
- Data pipelines
- Network protocols
- Real-time data processing

**Estimated effort:** 2-3 months
**Market gap:** Reactive Scheme is unexplored territory

---

#### 6. **Actor Model (Erlang style)** ⭐⭐⭐

**The Big Idea:** Lightweight processes with message passing

```scheme
; Spawn actor
(define counter-actor
  (spawn
    (lambda ()
      (let loop ((count 0))
        (receive
          [('increment) (loop (+ count 1))]
          [('get reply-to)
           (send reply-to count)
           (loop count)]
          [('reset) (loop 0)])))))

; Send messages
(send counter-actor '(increment))
(send counter-actor '(increment))
(send counter-actor `(get ,self))

(receive
  [(count) (display count)])  ; => 2

; Supervision trees
(define supervisor
  (spawn-supervisor
    'one-for-one
    [(worker-spec my-worker-fn)
     (worker-spec another-worker-fn)]))
```

**Why unique:**
- No Scheme has Erlang-style actors
- Rust has actix - can use as backend
- Perfect fit for Scheme's functional style

**Estimated effort:** 3-4 months
**Market gap:** Concurrent Scheme is rare

---

### Category D: Data Science & Numerics (LOW-MEDIUM Priority)

#### 7. **Dataframe Library (Polars style)** ⭐⭐⭐

**The Big Idea:** Leverage Rust's Polars for data science

```scheme
; Create dataframe
(define df
  (dataframe
    '((name . ["Alice" "Bob" "Carol"])
      (age . [25 30 35])
      (city . ["NYC" "LA" "SF"]))))

; Query
(dataframe-filter df
  (lambda (row) (> (get row 'age) 28)))

; Group by
(dataframe-group-by df 'city
  (lambda (group)
    (mean (get group 'age))))

; Join
(dataframe-join df other-df 'name)

; Lazy evaluation
(define lazy-df
  (-> df
      (select '(name age))
      (filter (lambda (row) (> (get row 'age) 25)))
      (sort-by 'age)
      (lazy)))

(collect lazy-df)  ; Execute query plan
```

**Why unique:**
- No Scheme has modern dataframe support
- Rust's Polars is fast and featureful
- Zero-copy FFI makes this practical

**Estimated effort:** 2-3 weeks (thin wrapper over Polars)
**Market gap:** Data science in Scheme is unexplored

---

#### 8. **Numeric Array Library (NumPy style)** ⭐⭐⭐

**The Big Idea:** Efficient numeric arrays via Rust ndarray

```scheme
; Create arrays
(define a (array [1 2 3 4 5]))
(define b (array [[1 2] [3 4]]))

; Operations (SIMD optimized)
(array-add a 10)  ; => [11 12 13 14 15]
(array-mul a 2)   ; => [2 4 6 8 10]

; Matrix operations
(array-dot matrix1 matrix2)
(array-transpose matrix)

; Broadcasting
(array-add matrix scalar)

; Slicing
(array-slice a 1 3)  ; => [2 3]
```

**Estimated effort:** 2-3 weeks
**Market gap:** Scientific computing in Scheme

---

### Category E: Developer Experience (HIGH Priority)

#### 9. **Language Server Protocol (LSP)** ⭐⭐⭐⭐⭐

**The Big Idea:** First-class IDE support

**Features:**
- **Autocomplete** - Context-aware suggestions
- **Go to definition** - Jump to function/variable definition
- **Hover documentation** - Show type and docs on hover
- **Error diagnostics** - Real-time error checking
- **Refactoring** - Rename symbols, extract functions
- **Code formatting** - Automatic code formatting

**Why unique:**
- Steel has LSP (good example to follow)
- Most Schemes have poor IDE support
- Modern developers expect LSP

**Implementation:**
```rust
// Leverage tower-lsp crate
use tower_lsp::*;

struct PatinaLanguageServer {
    interpreter: Arc<Mutex<Interpreter>>,
    // ...
}

impl LanguageServer for PatinaLanguageServer {
    async fn completion(&self, params: CompletionParams) -> Result<...> {
        // Return completions based on current environment
    }

    async fn hover(&self, params: HoverParams) -> Result<...> {
        // Return type and documentation
    }
}
```

**Estimated effort:** 3-4 weeks
**Market gap:** Good LSP for Scheme is rare

---

#### 10. **Package Manager (Cargo style)** ⭐⭐⭐⭐

**The Big Idea:** Modern package management

```scheme
; patina.toml
[package]
name = "my-app"
version = "0.1.0"
authors = ["Your Name"]

[dependencies]
http-client = "0.2"
json = "1.0"

; Command line
$ patina add sxml
$ patina build
$ patina test
$ patina publish
```

**Features:**
- Semantic versioning
- Dependency resolution
- Lock files
- Private registries
- Build scripts

**Why unique:**
- Snow (R7RS package manager) is basic
- Racket's raco is good but Racket-specific
- Cargo-style UX is gold standard

**Estimated effort:** 4-6 weeks
**Market gap:** Modern package management for Scheme

---

### Category F: Novel/Experimental (LOW Priority, HIGH Impact)

#### 11. **Time-Travel REPL** ⭐⭐⭐⭐⭐

**The Big Idea:** Rewind and replay REPL history

```scheme
patina> (define x 10)
patina> (set! x 20)
patina> (+ x 5)
25

patina> (repl-back)  ; Go back one step
patina> x
20

patina> (repl-back 2)  ; Go back to beginning
patina> x
10

patina> (repl-forward)
patina> x
20

patina> (repl-history)
0: (define x 10)
1: (set! x 20)
2: (+ x 5)

patina> (repl-jump 1)  ; Jump to step 1
patina> x
20
```

**Why unique:**
- **No REPL does this well**
- Racket stepper is close but not REPL-integrated
- Incredibly useful for learning and debugging

**Implementation:**
- Record all eval steps
- Clone environment at each step
- Allow navigation through history

**Estimated effort:** 2-3 weeks
**Market gap:** Completely novel

---

#### 12. **Provenance Tracking** ⭐⭐⭐

**The Big Idea:** Track where data came from

```scheme
; Every value remembers its origin
patina> (define x (+ 1 2))
patina> (provenance x)
{
  expression: (+ 1 2)
  file: "repl"
  line: 1
  timestamp: 2025-11-09T10:30:00Z
}

; Track transformations
patina> (define y (* x 10))
patina> (provenance y)
{
  expression: (* x 10)
  dependencies: [x: (+ 1 2)]
  ...
}

; Useful for debugging data pipelines
patina> (define result (process-data input))
patina> (trace-back result)
result <- (process-data input)
  input <- (read-file "data.csv")
    "data.csv" <- (user-input)
```

**Why unique:**
- **No language does this**
- Research topic (provenance-aware systems)
- Extremely useful for data science

**Estimated effort:** 3-4 months (research heavy)
**Market gap:** Novel research direction

---

## Prioritization Matrix

| Feature | Uniqueness | Effort | Value | Priority |
|---------|-----------|--------|-------|----------|
| **Rust FFI** | ⭐⭐⭐⭐⭐ | 2-3 months | ⭐⭐⭐⭐⭐ | **VERY HIGH** |
| **LSP** | ⭐⭐⭐⭐ | 1 month | ⭐⭐⭐⭐⭐ | **VERY HIGH** |
| **Gradual Typing** | ⭐⭐⭐⭐⭐ | 3-4 months | ⭐⭐⭐⭐⭐ | **HIGH** |
| **WASM Target** | ⭐⭐⭐⭐ | 1 month | ⭐⭐⭐⭐ | **HIGH** |
| **Time-Travel REPL** | ⭐⭐⭐⭐⭐ | 2-3 weeks | ⭐⭐⭐⭐ | **HIGH** |
| **Package Manager** | ⭐⭐⭐ | 1-2 months | ⭐⭐⭐⭐ | **MEDIUM-HIGH** |
| **Reactive Streams** | ⭐⭐⭐⭐ | 2-3 months | ⭐⭐⭐⭐ | **MEDIUM-HIGH** |
| **Dataframes** | ⭐⭐⭐⭐ | 2-3 weeks | ⭐⭐⭐ | **MEDIUM** |
| **Actor Model** | ⭐⭐⭐ | 3-4 months | ⭐⭐⭐ | **MEDIUM** |
| **Effect System** | ⭐⭐⭐⭐⭐ | 4-6 months | ⭐⭐⭐ | **MEDIUM** |
| **Array Library** | ⭐⭐⭐ | 2-3 weeks | ⭐⭐⭐ | **LOW-MEDIUM** |
| **Provenance** | ⭐⭐⭐⭐⭐ | 3-4 months | ⭐⭐⭐ | **LOW** |

---

## Recommended Feature Roadmap

### Phase 1 Complete (Current)
- R7RS compliance
- TCO
- I/O and exceptions
- Hook/debugger system

### Phase 2A: Developer Experience (3-4 months)
**Goal:** Make Patina the best Scheme for daily development

1. **LSP Implementation** (1 month)
   - Autocomplete, go-to-definition, hover docs
   - VSCode extension
   - Emacs mode integration

2. **Rust FFI Foundation** (2 months)
   - Basic function registration
   - Type conversions
   - Zero-copy for common types
   - Documentation and examples

3. **Time-Travel REPL** (2-3 weeks)
   - History recording
   - Back/forward navigation
   - Huge wow factor!

**Why this order:**
- LSP makes development pleasant
- FFI enables real-world use cases
- Time-travel REPL is a unique selling point

---

### Phase 2B: Gradual Typing (3-4 months)
**Goal:** Performance and correctness

1. **Type Annotations** (1 month)
   - Syntax and parser
   - Basic type checking

2. **Type Inference** (2 months)
   - Hindley-Milner
   - Contract generation

3. **Optimizations** (1 month)
   - Monomorphization
   - Inline numeric ops
   - Remove runtime checks

**Synergy:** Gradual typing + LSP = great IDE experience

---

### Phase 3: WASM & Package Manager (2-3 months)
**Goal:** Distribution and deployment

1. **WASM Compilation** (1 month)
   - Compile interpreter to WASM
   - Browser integration
   - Examples and docs

2. **Package Manager** (1-2 months)
   - Basic cargo-style package manager
   - Dependency resolution
   - Registry

**Why:** Enable web deployment and ecosystem growth

---

### Phase 4: Reactive & Concurrent (3-4 months)
**Goal:** Modern concurrency

1. **Reactive Streams** (2 months)
   - Stream primitives
   - Async integration
   - Backpressure

2. **Actor Model** (2 months) OR **Effect System** (4 months)
   - Pick based on user feedback

**Why:** Differentiate from traditional Schemes

---

### Phase 5+: Specialized Features
- Dataframes (if data science users)
- Effect system (if type system users)
- Provenance (if research interest)

---

## Competitive Positioning

**Patina's Niche:**

> "The modern, embeddable Scheme with excellent tooling and gradual typing"

**Compared to:**

| Scheme | Strength | Weakness | Patina's Advantage |
|--------|----------|----------|-------------------|
| **Steel** | Rust embedding | Limited R7RS, complex contracts | Better R7RS, simpler FFI |
| **Guile** | Mature, C FFI | C-centric, old tooling | Rust-native, modern LSP |
| **Racket** | Great IDE, typed racket | Heavy, slow gradual typing | Fast gradual typing, lightweight |
| **Chez** | Very fast | No gradual typing, minimal tooling | Types + LSP |
| **Chibi** | R7RS reference | Basic features only | Advanced features on R7RS base |

---

## Marketing Angles

### For Rust Developers
**"The scripting language for Rust projects"**
- Zero-copy FFI
- Type safety
- Async/await integration
- Cargo-like tooling

### For Scheme Enthusiasts
**"R7RS with modern tooling and types"**
- Full R7RS compliance
- Optional gradual typing
- Excellent LSP support
- Time-travel debugging

### For Data Scientists
**"Scheme meets Polars and NumPy"**
- DataFrame support
- Numeric arrays
- REPL-driven analysis
- Gradual typing for correctness

### For Web Developers
**"Scheme in the browser"**
- WASM compilation
- Reactive streams
- Modern package manager
- Interactive docs

---

## The Killer Application: Notebooks + Data Science

**See:** `PRD/NOTEBOOK_DATA_SCIENCE_VISION.md` for complete vision

**The Big Idea:**
Combine your **existing notebook designs** with **Rust-powered data science tools** to create the **best functional data science notebook**.

**Why this is a killer combination:**

1. **S-expression notebooks** (already designed in PRD/future/phase4/)
   - Notebooks are valid Scheme programs (can be loaded as libraries!)
   - Git-friendly text format, not JSON
   - Terminal-based with vim keybindings
   - Dependency tracking between cells

2. **Rust data tools via FFI** (Polars, ndarray)
   - DataFrames: 5-10x faster than Pandas
   - Numeric arrays: NumPy-like with SIMD optimization
   - Zero-copy FFI = native speed
   - Plotting via Rust plotters

3. **No one else has this combination:**
   - Jupyter: Python-centric, JSON format, browser-based
   - Observable: JavaScript-centric, cloud-based
   - Org-mode: Limited data tools
   - **Patina: Scheme + Rust speed + Terminal UX**

**Example:**
```scheme
;; sales-analysis.scm.nb - Valid Scheme file!
(notebook
  (cell code
    (define sales (dataframe-from-csv "sales.csv")))

  (cell code
    (-> sales
        (dataframe-group-by 'region
          (lambda (g) (mean (get g 'revenue))))
        (dataframe-sort-by 'total 'desc)))

  (cell code
    (plot-bar by-region #:save "revenue.png")))

;; Load as library:
(import (sales-analysis))
(use sales)  ; Access dataframe from other notebooks!
```

**Estimated effort:** 3-4 months
**Payoff:** Unique position in data science + Scheme communities

---

## Conclusion

**Top Recommendations for Uniqueness:**

1. **Rust FFI** (MUST HAVE) ⭐⭐⭐⭐⭐
   - Foundation for everything else
   - Enables data science tools
   - Differentiates from all Schemes except Steel
   - **Start after Phase 1 complete**

2. **Notebooks + Data Science** (KILLER APP) ⭐⭐⭐⭐⭐
   - Terminal-based data science notebooks
   - S-expression format (notebooks as programs)
   - Polars dataframes + ndarray arrays
   - **No one else is doing this**
   - **Start after FFI (depends on it)**

3. **LSP + Time-Travel REPL** (DEV EXPERIENCE) ⭐⭐⭐⭐⭐
   - Best-in-class developer experience
   - No Scheme has both
   - Time-travel is unique and impressive
   - **Can start in parallel with FFI**

4. **Gradual Typing** (Already Planned) ⭐⭐⭐⭐⭐
   - Unique if done well (fast, unlike Typed Racket)
   - Enables optimizations for data pipelines
   - Synergy with LSP (type-aware autocomplete)
   - **Phase 2 after FFI+LSP**

This combination would make Patina:

- **The best Scheme for data science** (Notebooks + Polars/ndarray) 🔬
- **The best Scheme for embedding in Rust** (FFI) 🦀
- **The best Scheme for daily development** (LSP + time-travel) 💻
- **The best terminal-based notebook** (Better than Jupyter for terminal users) 📊
- **A competitive alternative to Typed Racket** (Better performance) ⚡

**Positioning:**
> "Patina: Functional data science in your terminal, powered by Rust"

**Target Markets:**
- Data scientists tired of Jupyter/Pandas
- Functional programming enthusiasts
- Terminal power users
- Researchers needing reproducibility
- Rust developers wanting scripting

**Estimated timeline:**
- **To "uniquely compelling":** 6-9 months (FFI + LSP + Notebooks MVP)
- **To "feature complete":** 18-24 months (+ Gradual typing + Advanced features)

**Immediate next steps:**
1. Complete Phase 1 (R7RS compliance) - Current focus
2. Implement Rust FFI (2-3 months) - Foundation
3. Build Notebook MVP with dataframes (3-4 months) - Killer app
4. Launch with tutorial and example gallery
