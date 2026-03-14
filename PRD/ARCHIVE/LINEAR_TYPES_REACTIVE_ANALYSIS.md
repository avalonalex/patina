# Linear/Affine Types in Reactive Programming

**Last Updated:** 2025-11-09
**Status:** Research and Design

---

## The Question

How do linear types and affine types interact with reactive programming (specifically reactive streams in Patina)?

**TL;DR:** Linear/affine types and reactive programming are **highly synergistic** - they solve complementary problems and combining them enables powerful guarantees about resource safety, protocol compliance, and concurrent behavior.

---

## Background: What Are Linear and Affine Types?

### Linear Types

**Definition:** Every value must be used **exactly once**

```scheme
; Conceptual example (not real Patina syntax yet)
(: file-handle (Linear FileHandle))

; VALID: Used exactly once
(define handle (open-file "data.txt"))
(close-file handle)  ; handle consumed here

; INVALID: Used twice
(define handle (open-file "data.txt"))
(close-file handle)
(close-file handle)  ; ERROR: handle already consumed!

; INVALID: Never used
(define handle (open-file "data.txt"))
; ERROR: handle never consumed (resource leak!)
```

**Benefits:**
- **No resource leaks** - Must use every resource
- **No use-after-free** - Can't use after consuming
- **Deterministic cleanup** - Clear ownership transfer

---

### Affine Types

**Definition:** Every value must be used **at most once** (0 or 1 times)

```scheme
; Affine is more lenient - allows dropping without use
(define handle (open-file "data.txt"))
; OK to not use it (though wasteful)

; But still prevents double-use
(close-file handle)
(close-file handle)  ; ERROR: already consumed!
```

**Rust uses affine types** (not strictly linear):
- Move semantics = affine
- Can drop without using (destructor runs)
- Can't use after move

---

## Reactive Programming Primer

### What is Reactive Programming?

```scheme
; Stream of events over time
(define clicks (event-stream))  ; Mouse clicks
(define numbers (stream-range 0 infinity))  ; 0, 1, 2, ...

; Transform streams
(define doubled
  (stream-map (lambda (x) (* x 2)) numbers))

; Combine streams
(define merged
  (stream-merge clicks keypresses))

; Subscribe to stream
(stream-subscribe! clicks
  (lambda (event) (display "Clicked!")))
```

**Key challenges:**
1. **Resource management** - When to close streams?
2. **Subscription lifecycle** - When to unsubscribe?
3. **Backpressure** - What if producer is faster than consumer?
4. **Concurrent access** - Multiple subscribers, thread safety
5. **Protocol compliance** - Request before subscribe, etc.

---

## The Synergy: Linear Types + Reactive Streams

### Problem 1: Resource Leaks

**Without linear types:**
```scheme
; Easy to leak subscriptions!
(define stream (create-stream))
(stream-subscribe! stream handler)
; ... forgot to unsubscribe!
; Stream keeps running, consuming resources
```

**With linear types:**
```scheme
(: Subscription (Linear Subscription))

; Subscription must be consumed (unsubscribed)
(define sub (stream-subscribe stream handler))
; ... do work ...
(unsubscribe sub)  ; MUST call this, enforced by type system

; Or use RAII pattern:
(with-subscription stream handler
  (lambda ()
    ; ... do work ...
    ; Automatically unsubscribes when leaving scope
    ))
```

**Benefit:** Type system prevents subscription leaks!

---

### Problem 2: Use-After-Close Errors

**Without linear types:**
```scheme
(define stream (create-stream))
(stream-close! stream)
(stream-emit! stream 42)  ; BUG: Using closed stream!
; Runtime error or undefined behavior
```

**With linear types:**
```scheme
(: Stream (Linear (Stream T)))

(define stream (create-stream))
(stream-emit! stream 42)   ; OK
(stream-close stream)      ; Consumes stream
(stream-emit! stream 42)   ; COMPILE ERROR: stream already consumed!
```

**Benefit:** Impossible to use closed streams!

---

### Problem 3: Protocol Compliance (Session Types)

**Session types** (built on linear types) ensure protocols are followed:

```scheme
; Protocol: Must request before receiving
(: Connection (Session (Request -> Response -> End)))

; VALID:
(define conn (connect server))
(request conn "GET /data")
(define resp (receive conn))
(close conn)

; INVALID: Wrong order
(define conn (connect server))
(define resp (receive conn))  ; ERROR: Must request first!

; INVALID: Missing close
(define conn (connect server))
(request conn "GET /data")
(receive conn)
; ERROR: Connection not closed!
```

**In reactive context:**
```scheme
; Protocol for reactive streams:
; Create -> Subscribe -> (Emit* | Complete | Error) -> Unsubscribe

(: ReactiveStream (Session (
  Create ->
  Subscribe ->
  Choice (
    Emit -> ReactiveStream |  ; Recursive: can emit multiple times
    Complete -> End |
    Error -> End))))

; Type system enforces this protocol!
```

**Benefit:** Can't violate stream lifecycle at compile time!

---

### Problem 4: Ownership and Concurrent Streams

**Rust's approach (affine types + ownership):**

```rust
// Rust's tokio streams
let stream = futures::stream::iter(vec![1, 2, 3, 4, 5]);

// Can't share stream between threads without explicit sync
let handle1 = tokio::spawn(async move {
    stream.for_each(|x| async move { println!("{}", x) }).await;
});

// stream.for_each(...); // ERROR: stream moved above!
```

**In Patina with linear types:**
```scheme
(: Stream (Linear (Stream T)))

; Must explicitly split or clone for concurrent access
(define stream (create-stream))

; Option 1: Split ownership
(define (stream-a stream-b) (stream-split stream))
(spawn (process-stream stream-a))
(spawn (process-stream stream-b))

; Option 2: Explicit sharing (requires synchronization)
(define shared-stream (stream-make-shared stream))  ; Wraps in mutex
(spawn (process-shared-stream shared-stream))
(spawn (process-shared-stream shared-stream))
```

**Benefit:** Explicit about concurrent access, no data races!

---

## Practical Applications in Patina

### 1. Reactive Streams with Resource Safety

**Design:**
```scheme
(define-library (patina reactive)
  (export
    stream-create    ; -> (Linear (Stream T))
    stream-emit!     ; (Linear (Stream T)) T -> (Linear (Stream T))
    stream-subscribe ; (Linear (Stream T)) (T -> Void) -> (Linear (Stream T), Linear Subscription)
    stream-close     ; (Linear (Stream T)) -> Void
    subscription-cancel ; (Linear Subscription) -> Void
    )

  (begin
    ; Stream is linear - must be closed
    (: stream-create (-> (Linear (Stream T))))
    (define (stream-create) ...)

    ; Emit consumes and returns stream (threading state)
    (: stream-emit! (-> (Linear (Stream T)) T (Linear (Stream T))))
    (define (stream-emit! stream value)
      ; ... implementation ...
      stream)  ; Return stream for further use

    ; Subscribe returns both stream and subscription (both linear!)
    (: stream-subscribe (-> (Linear (Stream T))
                            (-> T Void)
                            (Tuple (Linear (Stream T)) (Linear Subscription))))
    (define (stream-subscribe stream handler)
      ; ... implementation ...
      (tuple stream subscription))

    ; Close consumes stream
    (: stream-close (-> (Linear (Stream T)) Void))
    (define (stream-close stream)
      ; ... cleanup ...
      (void))

    ; Unsubscribe consumes subscription
    (: subscription-cancel (-> (Linear Subscription) Void))
    (define (subscription-cancel sub)
      ; ... cleanup ...
      (void))))
```

**Usage:**
```scheme
(import (patina reactive))

; Create stream (linear)
(define stream (stream-create))

; Subscribe (both stream and subscription are linear)
(define-values (stream sub)
  (stream-subscribe stream
    (lambda (x) (display x) (newline))))

; Emit value (threading the stream through)
(define stream (stream-emit! stream 42))
(define stream (stream-emit! stream 43))

; Must clean up both!
(subscription-cancel sub)
(stream-close stream)

; Can't use stream or sub after this - type error!
```

---

### 2. Backpressure with Linear Types

**Problem:** Producer faster than consumer → unbounded buffering → memory exhaustion

**Solution:** Linear types + demand-driven streams

```scheme
; Consumer explicitly requests values
(define-library (patina reactive-backpressure)
  (export
    stream-request   ; Request N items
    stream-receive   ; Receive one item
    )

  (begin
    ; Protocol: Must request before receiving
    (: Demand (Linear Demand))
    (: stream-request (-> (Linear (Stream T)) Integer (Tuple (Linear (Stream T)) (Linear Demand))))
    (: stream-receive (-> (Linear Demand) (Tuple T (Linear Demand))))

    (define (stream-request stream n)
      ; ... create demand token for n items ...
      (tuple stream demand))

    (define (stream-receive demand)
      ; ... consume one item, return new demand ...
      (if (demand-remaining? demand)
          (tuple value new-demand)
          (error "No more demand!")))))
```

**Usage:**
```scheme
; Request 10 items
(define-values (stream demand) (stream-request stream 10))

; Receive items (demand is threaded through)
(define-values (val1 demand) (stream-receive demand))
(define-values (val2 demand) (stream-receive demand))
; ...
(define-values (val10 demand) (stream-receive demand))

; Can't receive more without requesting!
; (stream-receive demand)  ; ERROR: demand exhausted!
```

**Benefit:** Bounded buffers enforced by type system!

---

### 3. Hot vs Cold Streams

**Cold streams:** Each subscriber gets its own execution
**Hot streams:** All subscribers share one execution

**With linear types:**
```scheme
; Cold stream: Linear, single subscriber
(: ColdStream (Linear (ColdStream T)))

; Hot stream: Can be shared (internally synchronized)
(: HotStream (Shared (HotStream T)))

; Convert cold to hot (explicit)
(: stream-make-hot (-> (Linear (ColdStream T)) (Shared (HotStream T))))
(define (stream-make-hot cold-stream)
  ; ... wrap in synchronization ...
  hot-stream)

; Usage
(define cold (stream-create-cold))
(define hot (stream-make-hot cold))  ; Consume cold stream

; Now can share
(stream-subscribe hot handler1)
(stream-subscribe hot handler2)  ; OK - hot is shared
```

---

### 4. Observable Pattern with Ownership

**Classic problem:** Who owns the observable? When to clean up?

**With linear types:**
```scheme
(define-library (patina observable)
  (export
    observable-create
    observable-subscribe
    observable-dispose
    )

  (begin
    (: Observable (Linear (Observable T)))
    (: Observer (Linear Observer))

    ; Create observable with source function
    (: observable-create (-> (-> Observer Void) (Linear (Observable T))))
    (define (observable-create source-fn) ...)

    ; Subscribe returns both observable and subscription
    (: observable-subscribe
       (-> (Linear (Observable T))
           (-> T Void)
           (Tuple (Linear (Observable T)) (Linear Subscription))))
    (define (observable-subscribe obs handler)
      ; ... implementation ...
      (tuple obs subscription))

    ; Dispose consumes observable
    (: observable-dispose (-> (Linear (Observable T)) Void))
    (define (observable-dispose obs) ...)))
```

**Example:**
```scheme
; Create observable from interval
(define obs
  (observable-create
    (lambda (observer)
      ; Emit values every second
      (let loop ((n 0))
        (observer-next observer n)
        (sleep 1000)
        (loop (+ n 1))))))

; Subscribe
(define-values (obs sub)
  (observable-subscribe obs
    (lambda (x)
      (display "Received: ")
      (display x)
      (newline))))

; ... later ...
(subscription-cancel sub)
(observable-dispose obs)  ; Must dispose!
```

**Benefit:** Clear ownership, guaranteed cleanup!

---

## Integration with Rust's Tokio (via FFI)

### Rust's Approach to Reactive Safety

Rust's tokio uses **affine types** (ownership) for resource safety:

```rust
// Rust example
use tokio::sync::mpsc;

let (tx, mut rx) = mpsc::channel(100);

// tx is moved into task - can't use after
tokio::spawn(async move {
    tx.send(42).await.unwrap();
    // tx dropped here automatically
});

// rx is moved
while let Some(msg) = rx.recv().await {
    println!("Got: {}", msg);
}
// rx dropped here automatically
```

**Key insights:**
1. Ownership prevents use-after-drop
2. Move semantics make ownership transfer explicit
3. Async makes resource lifetime clear

---

### Patina FFI with Rust Reactive Streams

**Strategy:** Wrap Rust's tokio streams with linear types in Scheme

```scheme
(define-library (patina tokio-stream)
  (import (patina ffi))

  (export
    tokio-stream-create
    tokio-stream-next
    tokio-stream-close
    )

  (begin
    ; FFI declarations
    (define-ffi tokio-stream-create "tokio_stream_create"
      (-> (Linear RustStream)))

    (define-ffi tokio-stream-next "tokio_stream_next"
      (-> (Linear RustStream) (Tuple (Option Value) (Linear RustStream))))

    (define-ffi tokio-stream-close "tokio_stream_close"
      (-> (Linear RustStream) Void))

    ; Scheme wrappers maintain linearity
    (: stream-create (-> (Linear Stream)))
    (define (stream-create)
      (tokio-stream-create))

    (: stream-next (-> (Linear Stream) (Tuple (Option Value) (Linear Stream))))
    (define (stream-next stream)
      (tokio-stream-next stream))

    (: stream-close (-> (Linear Stream) Void))
    (define (stream-close stream)
      (tokio-stream-close stream))))
```

**Benefits:**
- Rust's runtime safety + Scheme's high-level API
- Linear types ensure Rust resources cleaned up
- Zero-cost abstraction (linear types compile away)

---

## Research: Linear Temporal Logic + Reactive Programming

**Fascinating connection:** Linear Temporal Logic (LTL) + FRP

From research (LTL types FRP):
> "Linear-time Temporal Logic is a natural extension of the type system for FRP, which constrains the temporal behaviour of reactive programs"

**What this means:**
- LTL can express properties like "eventually X happens" or "X happens until Y"
- Type system can verify these temporal properties
- Reactive programs = proofs of temporal properties!

**Example temporal properties:**
```scheme
; Type expresses temporal guarantee:
; "Stream will eventually complete or error"
(: Stream (Eventually (Choice Complete Error)))

; "After request, must receive response"
(: request (-> Request (Eventually Response)))

; "Temperature sensor emits values continuously"
(: temperature-sensor (-> (Always Temperature)))
```

**This is cutting-edge research!** But shows the deep connection between:
- Linear logic (linear types)
- Temporal logic (time/events)
- Reactive programming (event streams)

---

## Challenges and Trade-offs

### Challenge 1: Ergonomics

**Problem:** Threading linear values is verbose

```scheme
; Verbose: must thread stream through
(define stream (stream-create))
(define stream (stream-emit! stream 1))
(define stream (stream-emit! stream 2))
(define stream (stream-emit! stream 3))
(stream-close stream)
```

**Solutions:**

**Option 1: Monadic do-notation**
```scheme
(do-linear
  [stream <- stream-create]
  [stream <- stream-emit! stream 1]
  [stream <- stream-emit! stream 2]
  [stream <- stream-emit! stream 3]
  [() <- stream-close stream]
  (return ()))
```

**Option 2: Implicit threading macro**
```scheme
(with-linear-stream stream
  (emit! 1)
  (emit! 2)
  (emit! 3))
; Automatically closes
```

---

### Challenge 2: Compatibility with Existing Code

**Problem:** Most Scheme code doesn't use linear types

**Solution:** Gradual linearity (like gradual typing)

```scheme
; Untyped code - no guarantees
(define stream (stream-create))
; ... might leak

; Linearly typed code - compiler enforces
(: stream-create (-> (Linear Stream)))
(define stream (stream-create))
; ... must close

; Escape hatch: convert linear to non-linear (unsafe)
(define unrestricted-stream (linear->unrestricted stream))
; Now can use multiple times, but lose guarantees
```

---

### Challenge 3: Performance Overhead

**Question:** Do linear types add runtime overhead?

**Answer:** No! Linear types are **zero-cost abstractions**

- Type checking happens at compile time
- No runtime representation needed
- Compiled code identical to non-linear version
- Rust proves this works (ownership = affine types, zero overhead)

---

## Recommendation for Patina

### Phase 1: Build Reactive Streams without Linear Types

**Start simple:**
```scheme
(define-library (patina reactive)
  ; No linear types yet
  (export stream-create stream-emit stream-subscribe stream-close))
```

**Benefits:**
- Get reactive programming working first
- Learn what patterns emerge
- Identify resource management issues

---

### Phase 2: Add Affine Types (Rust-style Ownership)

**Adopt Rust's model:**
- Values are moved by default
- Must explicitly clone to share
- Destructors run automatically

**Syntax:**
```scheme
; Values are affine by default in typed code
(: stream Stream)
(define stream (stream-create))

; Move semantics
(process-stream stream)  ; Moves stream
; stream is invalid here

; Explicit clone to share
(define stream (stream-create))
(process-stream (stream-clone stream))
(process-stream stream)  ; OK, stream still valid
```

**Estimated effort:** 2-3 months (with gradual typing system)

---

### Phase 3: Add Full Linear Types (Optional)

**For advanced use cases:**
- Session types (protocol verification)
- Resource accounting (exactly-once processing)
- Temporal properties (LTL types)

**Estimated effort:** 4-6 months (research-heavy)

---

## Conclusion

### Linear/Affine Types + Reactive Programming = Powerful Combination

**Synergies:**
1. **Resource safety** - Prevent subscription/stream leaks
2. **Protocol compliance** - Enforce stream lifecycle
3. **Concurrency safety** - Explicit sharing, no data races
4. **Backpressure** - Demand-driven consumption
5. **Temporal properties** - Type-level guarantees about time

**Practical benefits:**
- No resource leaks (type-checked!)
- No use-after-close bugs
- Clear ownership semantics
- Compiler-verified correctness
- Zero runtime overhead

**Patina strategy:**
1. Build reactive streams first (Phase 3 of original plan)
2. Add affine types with gradual typing (Phase 2)
3. Optionally add linear types for advanced guarantees

**Key insight:** Rust has proven this works!
- Tokio's async streams use affine types
- Zero overhead
- Excellent ergonomics (with practice)
- Can adopt similar approach in Patina

This would make Patina the **first Scheme with type-safe reactive programming**! 🚀

---

## Further Reading

**Papers:**
- "Linear Temporal Logic Types for Functional Reactive Programming" (LTL types FRP)
- "Session Types as Intuitionistic Linear Propositions"
- "Implementing Multiparty Session Types in Rust"

**Implementations:**
- Rust tokio (affine types + async)
- Linear Haskell (linear types in Haskell)
- Session-Rust (session types in Rust)

**Patina design docs:**
- `PRD/UNIQUE_FEATURES_ANALYSIS.md` (gradual typing + reactive streams)
- `PRD/NOTEBOOK_DATA_SCIENCE_VISION.md` (reactive for data pipelines)
