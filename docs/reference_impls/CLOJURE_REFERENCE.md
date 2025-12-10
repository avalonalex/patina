# Clojure Reference

This document provides an analysis of Clojure's implementation, focusing on architectural patterns, data structures, and design decisions that may inform Patina's development. While Clojure is a Lisp dialect targeting the JVM (not R7RS Scheme), its innovative approaches to immutability, concurrency, and persistent data structures offer valuable insights.

**Note**: Clojure is a distinct Lisp dialect with different semantics than Scheme. This document focuses on implementation techniques and architectural ideas that transcend the language differences.

## Overview

**Repository**: `~/Project/reference/clojure`

Clojure consists of:
- **Java implementation** (`src/jvm/clojure/lang/`) - Core runtime, data structures, compiler (~139 Java files)
- **Embedded ASM library** (`src/jvm/clojure/asm/`) - Bytecode manipulation framework
- **Clojure standard library** (`src/clj/clojure/`) - Core functions and macros (~20+ .clj files)

Key architectural principle: **Immutable-first with JVM compilation**. Clojure compiles directly to JVM bytecode, with persistent data structures as the default.

## Repository Structure

```
~/Project/reference/clojure/
├── src/
│   ├── jvm/clojure/
│   │   ├── lang/          # Core Java implementation (139 files)
│   │   │   ├── Compiler.java        # 9679 lines, 301KB - Heart of Clojure
│   │   │   ├── RT.java              # 2414 lines - Runtime utilities
│   │   │   ├── PersistentHashMap.java  # HAMT implementation
│   │   │   ├── PersistentVector.java   # 32-ary tree vectors
│   │   │   ├── LazySeq.java         # Lazy sequence implementation
│   │   │   ├── Atom.java            # Lock-free mutable reference
│   │   │   ├── Ref.java             # STM reference
│   │   │   ├── Agent.java           # Async action queues
│   │   │   └── ...
│   │   ├── asm/           # Embedded ASM bytecode library
│   │   └── java/api/      # Java interop API
│   └── clj/clojure/       # Clojure standard library
│       ├── core.clj       # Core functions and macros
│       ├── core_print.clj # Printing implementation
│       └── ...
├── test/                  # Test suites
├── build.xml              # Ant build configuration
├── pom.xml                # Maven build configuration
└── changes.md             # Version history
```

## Key Files Reference

| File | Size | Purpose |
|------|------|---------|
| `Compiler.java` | 301KB | Compiler with special forms, code generation |
| `RT.java` | ~80KB | Runtime utilities, type conversions |
| `PersistentHashMap.java` | ~40KB | HAMT implementation |
| `PersistentVector.java` | ~35KB | 32-ary tree implementation |
| `EdnReader.java` | 21KB | EDN/reader implementation |
| `Namespace.java` | ~15KB | Namespace system |
| `LazySeq.java` | ~5KB | Lazy sequence implementation |
| `LockingTransaction.java` | ~20KB | STM implementation |

## Persistent Data Structures

### Hash Array Mapped Trie (HAMT)

**File**: `src/jvm/clojure/lang/PersistentHashMap.java`

Clojure's hash maps use Phil Bagwell's HAMT algorithm:

```java
// From PersistentHashMap.java comments:
// A persistent rendition of Phil Bagwell's Hash Array Mapped Trie
// - Uses path copying for persistence
// - HashCollision leaves vs. extended hashing
// - Node polymorphism vs. conditionals
// - No sub-tree pools or root-resizing
```

**Key characteristics:**
- **32-way branching**: Each node has up to 32 children (5 bits of hash per level)
- **Path copying**: Mutations create new nodes only along the modification path
- **Structural sharing**: Unchanged subtrees are shared between versions
- **O(log₃₂ n)** operations: ~7 levels for 1 billion entries

**Node types:**
- `ArrayNode`: Dense node with 32 slots
- `BitmapIndexedNode`: Sparse node with bitmap tracking populated slots
- `HashCollisionNode`: Handles hash collisions with linear search

**Creation pattern:**
```java
public static PersistentHashMap create(Object... init){
    ITransientMap ret = EMPTY.asTransient();
    for(int i = 0; i < init.length; i += 2){
        ret = ret.assoc(init[i], init[i + 1]);
    }
    return (PersistentHashMap) ret.persistent();
}
```

**Transient optimization**: Bulk updates use transient maps for O(1) mutations within an isolated scope, then convert back to persistent form.

**Actionable for Patina**: Consider HAMT for efficient persistent maps if Scheme-level association lists become a bottleneck.

### Persistent Vectors (32-ary Tree)

**File**: `src/jvm/clojure/lang/PersistentVector.java`

Clojure vectors use a 32-way branching tree with tail optimization:

```java
public class Node implements Serializable {
    transient public final AtomicReference<Thread> edit;  // Transient mutation tracking
    public final Object[] array;  // 32-element child array
}

final int cnt;           // Total element count
public final int shift;  // Current tree depth (* 5)
public final Node root;  // Tree root
public final Object[] tail;  // Last partial node (optimization)
```

**Key features:**
- **O(1) append**: Tail buffer avoids tree traversal for common case
- **O(1) lookup**: Direct index calculation via bit manipulation
- **Shift value**: 5 (2^5 = 32 elements per node)
- **Transient vectors**: Efficient batch construction

**Index calculation:**
```java
// For index i, navigate tree by extracting 5 bits at each level
int level = shift;
Node node = root;
while(level > 0) {
    node = (Node) node.array[(i >>> level) & 0x1f];
    level -= 5;
}
return node.array[i & 0x1f];
```

**Actionable for Patina**: Current `Vec<Value>` is simpler but requires full copy on mutation. Consider persistent vectors for Scheme vectors if immutability becomes important.

### Persistent Lists

**File**: `src/jvm/clojure/lang/PersistentList.java`

Traditional cons-cell linked list, similar to Scheme:
- O(1) cons (prepend)
- O(1) first
- O(n) count and nth
- `EmptyList` singleton for empty state

**Actionable for Patina**: Current Scheme list representation is similar.

## Lazy Sequences

**File**: `src/jvm/clojure/lang/LazySeq.java`

Clojure's lazy sequences are fundamental to its functional programming style:

```java
public final class LazySeq extends Obj implements ISeq, Sequential, List, ... {
    private transient IFn fn;           // Unevaluated function
    private Object sv;                   // Cached value (seq or list)
    private ISeq s;                      // Materialized sequence
    private Lock lock;                   // Thread-safe forcing
}
```

**Forcing mechanism:**
```java
final private void lockAndForce() {
    Lock l = lock;
    if(l != null) {
        l.lock();
        try {
            force();  // Invoke fn if not yet forced
        } finally {
            l.unlock();
        }
    }
}

final private Object sval() {
    if(fn != null)
        lockAndForce();  // Thread-safe first-time evaluation
    if(sv != null)
        return sv;  // Return cached result
    return s;
}
```

**Key features:**
- **Memoization**: Once forced, result cached in `sv` or `s`
- **Thread-safe**: Lock ensures safe concurrent forcing
- **Recursive unwrapping**: Handles nested `LazySeq` transparently
- **Chunked sequences**: 32-element chunks for reduced overhead

**Actionable for Patina**: Scheme's `delay`/`force` is similar but simpler. Clojure's chunking optimization is worth studying for performance-critical lazy operations.

### Chunked Sequences

Clojure optimizes lazy sequences with 32-element chunks:

**Interface**: `IChunkedSeq` with `chunkedFirst()` and `chunkedNext()`

**Purpose**: Reduce function call overhead by processing elements in batches.

```java
public interface IChunk extends Indexed {
    IChunk dropFirst();
    Object reduce(IFn f, Object start);
}
```

## Concurrency Primitives

Clojure provides multiple concurrency primitives for different use cases:

### Atoms (Lock-Free References)

**File**: `src/jvm/clojure/lang/Atom.java` (~80 lines)

Atoms provide synchronous, uncoordinated updates:

```java
public class Atom extends ARef implements IAtom2 {
    final AtomicReference state;

    public Object swap(IFn f) {
        for(;;) {  // Optimistic retry loop
            Object v = deref();
            Object newv = f.invoke(v);
            validate(newv);  // Optional validation
            if(state.compareAndSet(v, newv)) {
                notifyWatches(v, newv);
                return newv;
            }
            // Retry if CAS failed (another thread won)
        }
    }
}
```

**Key features:**
- **Compare-and-swap (CAS)**: Lock-free using `AtomicReference`
- **Retry semantics**: Function re-applied on contention
- **Watches**: Callback notification on state change
- **Validators**: Optional invariant checking

**Actionable for Patina**: This pattern is useful for implementing parameters or mutable state with thread safety.

### Refs (Software Transactional Memory)

**File**: `src/jvm/clojure/lang/Ref.java` (~368 lines)

Refs provide coordinated, synchronous updates across multiple references:

```java
public class Ref extends ARef implements IFn, Comparable<Ref>, IRef {
    TVal tvals;                          // Multi-version value history (MVCC)
    final AtomicInteger faults;
    final ReentrantReadWriteLock lock;
    LockingTransaction.Info tinfo;       // Current transaction info
}
```

**Transaction system** (`LockingTransaction.java`):

```java
// Transaction states
static final int RUNNING = 0;
static final int COMMITTING = 1;
static final int RETRY = 2;
static final int KILLED = 3;
static final int COMMITTED = 4;

// Transaction limits
public static final int RETRY_LIMIT = 10000;
public static final long BARGE_WAIT_NANOS = 10 * 1000000;
```

**MVCC (Multi-Version Concurrency Control)**:
- Each ref maintains a history of values (`TVal` linked list)
- Transactions read from consistent snapshot at their start time
- Conflict detection on commit
- Pessimistic locking with "barge" protocol for deadlock prevention

**Actionable for Patina**: STM is complex but powerful. Study this if implementing advanced concurrency in future phases.

### Agents (Asynchronous Actions)

**File**: `src/jvm/clojure/lang/Agent.java` (~336 lines)

Agents provide asynchronous, uncoordinated updates:

```java
public class Agent extends ARef {
    static class ActionQueue {
        public final IPersistentStack q;    // Action queue
        public final Throwable error;       // Fail state
    }

    // Two thread pools for different semantics
    volatile public static ExecutorService pooledExecutor =    // send: CPU-bound
        Executors.newFixedThreadPool(2 + Runtime.getRuntime().availableProcessors());
    volatile public static ExecutorService soloExecutor =      // send-off: I/O-bound
        Executors.newCachedThreadPool();

    volatile Keyword errorMode = CONTINUE;  // Or FAIL
    volatile IFn errorHandler;
}
```

**Key features:**
- Actions queued and executed serially per agent
- Two executor pools: fixed (CPU) vs cached (I/O)
- Error handling modes: continue or fail-fast
- Integration with transaction system (queued for commit)

## Compilation Model

### Overview

Clojure compiles to JVM bytecode, not interpreted:

```
Source Code
    ↓
[EdnReader] → Forms (Clojure data structures)
    ↓
[Compiler.analyze()] → Expression AST
    ↓
[Compiler.emit()] → JVM bytecode (via ASM)
    ↓
[JVM] → Execution
```

### Reader (EdnReader.java)

**File**: `src/jvm/clojure/lang/EdnReader.java` (21KB)

Character and dispatch macro system:

```java
static IFn[] macros = new IFn[256];           // Single-char macros
static IFn[] dispatchMacros = new IFn[256];   // Two-char macros (#char)

static {
    macros['"'] = new StringReader();
    macros[';'] = new CommentReader();
    macros['^'] = new MetaReader();
    macros['('] = new ListReader();
    macros['#'] = new DispatchReader();

    dispatchMacros['{'] = new SetReader();
    dispatchMacros['_'] = new DiscardReader();
    dispatchMacros[':'] = new NamespaceMapReader();
}
```

**Numeric literals support:**
- Integers: decimal, hex (0x), octal (0), radix (2r-36r), BigInt (N suffix)
- Rationals: exact fractions (N/D)
- Floats: decimal, scientific, BigDecimal (M suffix)

**Actionable for Patina**: Similar to Scheme's reader macros. The dispatch macro system allows extensible syntax.

### Compiler Architecture

**File**: `src/jvm/clojure/lang/Compiler.java` (9679 lines, 301KB)

The compiler is the heart of Clojure. Key components:

**Special Forms Registry:**
```java
static final public IPersistentMap specials = PersistentHashMap.create(
    DEF,      new DefExpr.Parser(),        // Variable definition
    LOOP,     new LetExpr.Parser(),        // Loop binding
    RECUR,    new RecurExpr.Parser(),      // Tail recursion
    IF,       new IfExpr.Parser(),         // Conditional
    CASE,     new CaseExpr.Parser(),       // Pattern matching
    LET,      new LetExpr.Parser(),        // Local binding
    LETFN,    new LetFnExpr.Parser(),      // Local function definition
    DO,       new BodyExpr.Parser(),       // Sequencing
    FN,       null,                        // Lambda (special handling)
    QUOTE,    new ConstantExpr.Parser(),   // Quote
    TRY,      new TryExpr.Parser(),        // Exception handling
    THROW,    new ThrowExpr.Parser(),      // Exception throwing
    DOT,      new HostExpr.Parser(),       // Java interop
    NEW,      new NewExpr.Parser(),        // Instance creation
    DEFTYPE,  new NewInstanceExpr.DeftypeParser(),
    REIFY,    new NewInstanceExpr.ReifyParser()
);
```

**Compilation context tracking (dynamic vars):**
- `LOCAL_ENV`: Lexical scope for local bindings
- `LOOP_LOCALS`: Loop variables for recur
- `LOOP_LABEL`: Target label for recur
- `METHOD_RETURN_CONTEXT`: Tail position tracking

**Compilation modes:**
```java
public enum C {
    STATEMENT,    // Value ignored (side effects only)
    EXPRESSION,   // Value required
    RETURN,       // Tail position (enables TCO via recur)
    EVAL          // Runtime evaluation
}
```

**Actionable for Patina**: Patina's CoreExpr IR serves a similar purpose to Clojure's expression AST, but targets tree-walking rather than bytecode.

### IFn Interface

Clojure's function interface avoids boxing for common arities:

```java
public interface IFn extends Callable, Runnable {
    Object invoke();
    Object invoke(Object arg1);
    Object invoke(Object arg1, Object arg2);
    // ... through invoke(20 args)
    Object invoke(Object... args);  // Varargs fallback
    Object applyTo(ISeq arglist);   // Apply for unknown arity
}
```

**21 overloads**: 0-20 positional arguments plus varargs. This avoids array allocation and boxing for common cases.

**Actionable for Patina**: Similar to Scheme's arity handling, but more aggressive optimization.

## Namespace System

**File**: `src/jvm/clojure/lang/Namespace.java`

```java
public class Namespace extends AReference implements Serializable {
    final public Symbol name;

    // Two-level mapping system
    transient final AtomicReference<IPersistentMap> mappings;  // Symbol → Var
    transient final AtomicReference<IPersistentMap> aliases;   // Symbol → Namespace

    // Global registry
    final static ConcurrentHashMap<Symbol, Namespace> namespaces;
}
```

**Key features:**
- **Symbol-to-Var mapping**: Each namespace maps symbols to Var objects
- **Aliases**: Namespace aliases for qualified references
- **Thread-safe interning**: Atomic creation of new Vars
- **Global registry**: All namespaces discoverable

**Interning pattern:**
```java
public Var intern(Symbol sym){
    IPersistentMap map = getMappings();
    Var v = null;

    while((o = map.valAt(sym)) == null){
        if(v == null)
            v = new Var(this, sym);
        IPersistentMap newMap = map.assoc(sym, v);
        mappings.compareAndSet(map, newMap);  // Atomic create-if-absent
        map = getMappings();
    }
    return (Var) o;
}
```

**Actionable for Patina**: Similar to Scheme's library system but with Vars as indirection layer.

## Dynamic Binding (Vars)

**File**: `src/jvm/clojure/lang/Var.java`

Clojure Vars support thread-local dynamic binding:

```java
static class Frame {
    final static Frame TOP = new Frame(PersistentHashMap.EMPTY, null);
    Associative bindings;  // Var → TBox (thread-specific value)
    Frame prev;
}

static final ThreadLocal<Frame> dvals = new ThreadLocal<Frame>(){
    protected Frame initialValue(){
        return Frame.TOP;
    }
};
```

**Usage in Clojure:**
```clojure
(def ^:dynamic *config* {:debug false})

(binding [*config* {:debug true}]
  ; Within this scope, *config* has thread-local value
  (do-something))
```

**Actionable for Patina**: Scheme's `parameterize` serves a similar purpose. Study Var implementation for efficient dynamic binding.

## Macro System (Deep Dive)

Clojure's macro system is procedural (not pattern-based like Scheme's `syntax-rules`), but shares the same high-level goal: code transformation at compile time.

### Macro Storage and Definition

**File**: `src/jvm/clojure/lang/Var.java`

Macros are stored as regular Vars with a `:macro` metadata flag:

```java
// Var.java lines 82-83, 245-251
static Keyword macroKey = Keyword.intern(null, "macro");

public void setMacro() {
    alterMeta(assoc, RT.list(macroKey, RT.T));
}

public boolean isMacro(){
    return RT.booleanCast(meta().valAt(macroKey));
}
```

**`defmacro` expands to:**
```clojure
(do
  (defn name [&form &env ...user-params...] ...)
  (. (var name) (setMacro))
  (var name))
```

Key insight: **Macros are just functions with a metadata flag**. The `&form` and `&env` parameters are automatically prepended.

### Macro Expansion Mechanism

**File**: `src/jvm/clojure/lang/Compiler.java` (lines 7484-7659)

**Step 1: Identify macros**
```java
static public Var isMacro(Object op) {
    // Local bindings can't be macros
    if(op instanceof Symbol && referenceLocal((Symbol) op) != null)
        return null;
    if(op instanceof Symbol || op instanceof Var) {
        Var v = (op instanceof Var) ? (Var) op : lookupVar((Symbol) op, false, false);
        if(v != null && v.isMacro()) {
            if(v.ns != currentNS() && !v.isPublic())
                throw new IllegalStateException("var: " + v + " is not public");
            return v;
        }
    }
    return null;
}
```

**Step 2: Single-step expansion (`macroexpand1`)**
```java
public static Object macroexpand1(Object x) {
    if(x instanceof ISeq) {
        ISeq form = (ISeq) x;
        Object op = RT.first(form);
        if(isSpecial(op))
            return x;  // Special forms are not expanded

        Var v = isMacro(op);
        if(v != null) {
            // KEY: Pass [form, LOCAL_ENV, ...args] to macro function
            ISeq args = RT.cons(form, RT.cons(Compiler.LOCAL_ENV.get(), form.next()));
            return v.applyTo(args);
        }
    }
    return x;
}
```

**Step 3: Recursive expansion**
```java
static Object macroexpand(Object form) {
    Object exf = macroexpand1(form);
    if(exf != form)
        return macroexpand(exf);  // Keep expanding until stable
    return form;
}
```

### &form and &env: Compile-Time Introspection

**`&form`** - The original macro call form:
- Preserves source location (line, column)
- Enables better error messages
- Can be used to inspect the literal syntax

**`&env`** - The compile-time lexical environment:
- Map of local symbols visible at call site
- Enables macros to inspect what's in scope
- Powerful for conditional code generation

```clojure
;; Example: macro that behaves differently based on what's in scope
(defmacro debug-if-enabled [expr]
  (if (contains? &env 'debug)
    `(when ~'debug (println "DEBUG:" '~expr "=" ~expr))
    expr))
```

**Actionable for Patina**: Currently Patina's macros don't receive environment information. Adding `&env` equivalent would enable:
- Better error messages with source locations
- Macros that adapt based on lexical context
- Optimization opportunities (inline if binding is known constant)

### Syntax-Quote Implementation

**File**: `src/jvm/clojure/lang/LispReader.java` (lines 991-1148)

Syntax-quote (backtick) is implemented as a **reader macro**, not a special form:

```java
// Reader registration
macros['`'] = new SyntaxQuoteReader();
macros['~'] = new UnquoteReader();
```

**Key behaviors:**

**1. Namespace qualification** (automatic hygiene):
```java
// Unqualified symbols get current namespace
sym = Symbol.intern(resolver.currentNS().name, sym.name);
ret = RT.list(Compiler.QUOTE, sym);
```

This means:
```clojure
;; In namespace user
`foo  ;; Expands to (quote user/foo)
```

**2. Auto-gensym** (`symbol#`):
```java
if(sym.ns == null && sym.name.endsWith("#")) {
    IPersistentMap gmap = (IPersistentMap) GENSYM_ENV.deref();
    Symbol gs = (Symbol) gmap.valAt(sym);
    if(gs == null) {
        // First occurrence: generate unique name
        GENSYM_ENV.set(gmap.assoc(sym, gs = Symbol.intern(null,
            sym.name.substring(0, sym.name.length() - 1)
            + "__" + RT.nextID() + "__auto__")));
    }
    sym = gs;
}
```

This means:
```clojure
`(let [x# 1] x#)
;; Expands to: (let [x__1234__auto__ 1] x__1234__auto__)
;; Same x# within one syntax-quote gets same generated name
```

**3. Collection expansion** (handles unquote inside):
```java
else if(form instanceof ISeq || form instanceof IPersistentList) {
    ISeq seq = RT.seq(form);
    if(seq == null)
        ret = RT.cons(LIST, null);
    else
        ret = RT.list(SEQ, RT.cons(CONCAT, sqExpandList(seq)));
}
```

**4. List element expansion**:
```java
private static ISeq sqExpandList(ISeq seq) {
    PersistentVector ret = PersistentVector.EMPTY;
    for(; seq != null; seq = seq.next()) {
        Object item = seq.first();
        if(isUnquote(item))
            ret = ret.cons(RT.list(LIST, RT.second(item)));     // ~x → (list x)
        else if(isUnquoteSplicing(item))
            ret = ret.cons(RT.second(item));                     // ~@xs → xs
        else
            ret = ret.cons(RT.list(LIST, syntaxQuote(item)));   // x → (list 'x)
    }
    return ret.seq();
}
```

### Clojure vs Scheme Hygiene: A Comparison

| Aspect | Clojure | Patina (Scheme) |
|--------|---------|-----------------|
| **Mechanism** | Auto-gensym + namespace qualification | Scope sets (Racket-style) |
| **Symbol naming** | `x#` → `x__N__auto__` | `x` with scope metadata |
| **Capture prevention** | Explicit: use `x#` for locals | Automatic: scope sets handle it |
| **Reference to outer** | Qualified: `user/foo` | Scope subset matching |
| **Flexibility** | Can break hygiene easily (`~'foo`) | Hygiene enforced by default |
| **Implementation** | ~150 lines in reader | ~500 lines in macro expander |

**Trade-offs:**

Clojure's approach:
- **Simpler implementation**: Just string manipulation + counter
- **Explicit control**: Programmer decides what needs gensym
- **Easy to break**: `~'foo` inserts literal symbol (intentional unhygiene)
- **Namespace-based**: Relies on namespace qualification

Scheme's scope sets:
- **Principled**: Based on lexical scoping theory
- **Automatic**: No manual gensym needed
- **Harder to break**: Hygiene is the default
- **More complex**: Requires scope tracking through expansion

### Error Handling with Source Locations

**File**: `src/jvm/clojure/lang/Compiler.java` (lines 7579-7610)

```java
catch(IllegalArgumentException | IllegalStateException | ExceptionInfo e) {
    throw new CompilerException(
        (String) SOURCE_PATH.deref(),
        lineDeref(),
        columnDeref(),
        (op instanceof Symbol ? (Symbol) op : null),
        CompilerException.PHASE_MACRO_SYNTAX_CHECK,
        e);
}
```

**Error phases:**
- `PHASE_MACRO_SYNTAX_CHECK`: Error in macro's logic
- `PHASE_MACROEXPANSION`: Error during expansion itself

**ArityException adjustment:**
```java
catch(ArityException e) {
    // Hide the 2 extra params (&form and &env)
    throw new ArityException(e.actual - 2, e.name);
}
```

**Actionable for Patina**: Currently macro errors in Patina don't include source locations. Adding form metadata tracking would improve error messages.

### What We Can Borrow for Patina

**1. &env for compile-time environment access**

Currently Patina's macro-aware desugarer has the environment but doesn't expose it to macros. We could:
```scheme
;; Hypothetical: macro that checks if binding exists
(define-syntax when-bound
  (lambda (stx env)  ; env parameter
    (syntax-case stx ()
      [(_ id body ...)
       (if (env-contains? env #'id)
           #'(begin body ...)
           #'(void))])))
```

**2. Source location preservation**

Clojure's `&form` preserves the original form's metadata (line, column). Patina could:
- Add source location to `Value::Identifier`
- Pass original form to macro transformer
- Use in error messages

**3. Explicit unhygienic escape**

Clojure's `~'symbol` allows intentional hygiene breaking. Patina's scope sets could support:
- A `datum->syntax` equivalent for intentional capture
- Clearer semantics than implicit capture

**4. Namespace qualification in quasiquote**

Clojure's syntax-quote automatically qualifies symbols. For Patina:
- Could qualify to current library in quasiquote
- Prevents accidental capture of library internals
- More explicit than relying solely on scope sets

### Example: Implementing `when` in Both Systems

**Clojure:**
```clojure
(defmacro when [test & body]
  `(if ~test
     (do ~@body)
     nil))

;; Expansion of (when (> x 0) (print x) x):
;; (if (> x 0) (do (print x) x) nil)
```

**Patina (syntax-rules):**
```scheme
(define-syntax when
  (syntax-rules ()
    [(_ test body ...)
     (if test (begin body ...) #f)]))

;; Expansion is similar, but hygiene is automatic
```

**Key difference**: In Clojure, if `test` or `body` contained a symbol `if`, you'd need to be careful. In Scheme with scope sets, the `if` in the template is automatically distinguished from any `if` at the use site.

## Protocols and Multimethods

### Multimethods (Dynamic Dispatch)

**File**: `src/jvm/clojure/lang/MultiFn.java`

```java
public class MultiFn extends AFn {
    final public IFn dispatchFn;            // Dispatch function
    final public Object defaultDispatchVal;
    final public IRef hierarchy;            // Type hierarchy

    volatile IPersistentMap methodTable;    // dispatchVal → method
    volatile IPersistentMap preferTable;    // preference constraints
    volatile IPersistentMap methodCache;    // Optimized lookups
}
```

**Usage:**
```clojure
(defmulti area :shape)
(defmethod area :circle [{:keys [r]}] (* Math/PI r r))
(defmethod area :rectangle [{:keys [w h]}] (* w h))
```

**Features:**
- Dispatch on arbitrary function result
- Hierarchy-aware (isa? relationships)
- Preference system for ambiguous dispatches
- Method caching for performance

### Protocols (Fast Type Dispatch)

Protocols provide faster dispatch than multimethods for type-based polymorphism:

```clojure
(defprotocol Drawable
  (draw [this]))

(extend-type Circle
  Drawable
  (draw [this] ...))
```

**Implementation**: Protocols compile to Java interfaces when possible, with fallback to method table lookup.

**Actionable for Patina**: Multimethods are similar to CLOS generic functions. Consider for future extensibility.

## Watch Mechanism

**File**: `src/jvm/clojure/lang/ARef.java`

References (Atoms, Refs, Agents) support watches:

```java
public abstract class ARef extends AReference implements IRef {
    volatile IPersistentMap watches;

    public IRef addWatch(Object key, IFn callback){
        watches = RT.assoc(watches, key, callback);
        return this;
    }

    protected void notifyWatches(Object oldVal, Object newVal){
        for(Object watch : watches.values()){
            ((IFn)watch).invoke(/*key*/, /*ref*/, oldVal, newVal);
        }
    }
}
```

**Actionable for Patina**: This pattern enables reactive programming. Study for future reactive streams phase.

## Numeric Tower

Clojure's numeric tower with automatic promotion:

**Types:**
- `long` - 64-bit integers (default)
- `double` - IEEE 754 floats
- `BigInteger` - Arbitrary precision
- `Ratio` - Exact fractions
- `BigDecimal` - Arbitrary precision decimals

**Promotion:**
```java
// From Numbers.java
public static Number add(long x, long y){
    long ret = x + y;
    if ((ret ^ x) < 0 && (ret ^ y) < 0)  // Overflow detection
        return add((Object)x, (Object)y);  // Promote to BigInteger
    return ret;
}
```

**Actionable for Patina**: Patina's numeric tower is similar, using `i64` → `BigInt` promotion.

## Comparison: Clojure vs Patina (Scheme)

| Aspect | Clojure | Patina (Scheme) |
|--------|---------|-----------------|
| **Target** | JVM bytecode | Tree-walker (planned: VM/JIT) |
| **Data Model** | Immutable-first, persistent collections | S-expressions, mutable by default |
| **Concurrency** | Atoms, Refs (STM), Agents | Planned: reactive streams |
| **Dispatch** | Multimethods, Protocols | Single dispatch, SRFI-style |
| **Namespaces** | First-class with Vars | Libraries (R7RS) |
| **Macros** | Procedural (syntax-quote) | Hygienic (syntax-rules, scope sets) |
| **Typing** | Dynamic | Planned: gradual typing |
| **Sequences** | Lazy by default, chunked | Eager lists, explicit delay/force |

## Key Takeaways for Patina

### Immediately Applicable

1. **Transient pattern**: For bulk updates to persistent structures, use temporary mutable form
2. **Watch mechanism**: Callback registration for reactive updates
3. **CAS retry loops**: Lock-free updates with optimistic concurrency

### Future Considerations

1. **HAMT implementation**: If hash tables become performance-critical
2. **Chunked lazy sequences**: For efficient lazy evaluation
3. **STM concepts**: If coordinated state updates are needed
4. **Dynamic binding with ThreadLocal**: For efficient parameter implementation

### Design Philosophy Differences

1. **Clojure**: "Immutable by default, explicit state management"
2. **Scheme**: "Minimal core, powerful macros, mutation available"

Clojure's aggressive immutability and explicit state management (Atoms, Refs, Agents) contrasts with Scheme's more permissive approach where `set!` is available everywhere.

## References

### Papers and Resources

1. **Hash Array Mapped Tries**:
   *Ideal Hash Trees* - Phil Bagwell, 2001

2. **Persistent Data Structures**:
   *Purely Functional Data Structures* - Chris Okasaki, 1998

3. **Software Transactional Memory**:
   Clojure's STM is inspired by Haskell's STM and database MVCC

### Code Locations

- **Core runtime**: `~/Project/reference/clojure/src/jvm/clojure/lang/`
- **Standard library**: `~/Project/reference/clojure/src/clj/clojure/`
- **Reader**: `src/jvm/clojure/lang/EdnReader.java`
- **Compiler**: `src/jvm/clojure/lang/Compiler.java`

---

**Document created**: 2025-12-04
**Clojure version analyzed**: Latest (main branch)
**Analysis focus**: Persistent data structures, concurrency primitives, compilation model, namespace system
