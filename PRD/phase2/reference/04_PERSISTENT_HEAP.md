# Persistent Heap with Lightweight Snapshotting

**Priority:** ⭐⭐ Medium-High
**Complexity:** Medium-High (4-6 weeks)
**Impact:** High (UX - time-travel debugging)
**Status:** Research

---

## Overview

Implement a copy-on-write persistent heap that allows the VM to take nearly-free snapshots of execution state. This enables time-travel debugging where users can scrub through execution history, inspect past states, and replay execution.

**Key Insight:** Scheme VMs are small enough that we can maintain execution history without prohibitive overhead, unlocking debugging superpowers.

---

## The Vision

**REPL with time-travel:**
```scheme
patina> (define x 10)
patina> (set! x (+ x 5))
patina> x
15

patina> (rewind 2)  ; ← Go back 2 evaluations
patina> x
10

patina> (replay)    ; ← Step forward
patina> x
15

patina> (snapshot-save "before-crash")
patina> (buggy-function)
Error: Division by zero

patina> (snapshot-restore "before-crash")
patina> (debug buggy-function)  ; ← Try again with debugger
```

---

## Implementation Strategy

### Phase 1: Structural Sharing Heap (2 weeks)

**Copy-on-write data structures:**

```rust
// Instead of mutable heap
pub struct Heap {
    objects: Vec<HeapObject>,
}

// Use persistent data structure
pub struct PersistentHeap {
    root: Arc<HeapNode>,
    generation: u64,
}

pub struct HeapNode {
    objects: im::HashMap<ObjectId, HeapObject>,  // Immutable hashmap
    parent: Option<Arc<HeapNode>>,
}

impl PersistentHeap {
    fn allocate(&self, obj: HeapObject) -> (Self, ObjectId) {
        // Create new heap version with structural sharing
        let mut new_objects = self.root.objects.clone();  // O(1) with im crate
        let obj_id = self.next_object_id();
        new_objects.insert(obj_id, obj);

        let new_heap = PersistentHeap {
            root: Arc::new(HeapNode {
                objects: new_objects,
                parent: Some(self.root.clone()),
            }),
            generation: self.generation + 1,
        };

        (new_heap, obj_id)
    }

    fn get(&self, obj_id: ObjectId) -> Option<&HeapObject> {
        // Walk back through versions
        let mut node = &self.root;
        loop {
            if let Some(obj) = node.objects.get(&obj_id) {
                return Some(obj);
            }
            node = node.parent.as_ref()?;
        }
        None
    }
}
```

**Benefits:**
- Snapshot = clone Arc pointer (O(1))
- Mutations create new version, share unchanged data
- Automatic GC via reference counting

---

### Phase 2: Snapshot Management (1 week)

```rust
pub struct SnapshotManager {
    snapshots: HashMap<String, Snapshot>,
    history: VecDeque<Snapshot>,
    max_history: usize,
}

pub struct Snapshot {
    heap: PersistentHeap,
    globals: im::HashMap<Symbol, Value>,
    timestamp: Instant,
    description: String,
}

impl SnapshotManager {
    fn take_snapshot(&mut self, vm: &VM, name: Option<String>) -> SnapshotId {
        let snapshot = Snapshot {
            heap: vm.heap.clone(),  // O(1)!
            globals: vm.globals.clone(),  // O(1) with im::HashMap
            timestamp: Instant::now(),
            description: name.unwrap_or_else(|| format!("Auto {}", self.history.len())),
        };

        self.history.push_back(snapshot);

        // Evict old snapshots (LRU)
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }

    fn restore_snapshot(&mut self, vm: &mut VM, id: SnapshotId) {
        let snapshot = &self.snapshots[&id];
        vm.heap = snapshot.heap.clone();
        vm.globals = snapshot.globals.clone();
    }
}
```

---

### Phase 3: REPL Integration (1 week)

```rust
impl REPL {
    fn eval_with_snapshot(&mut self, expr: &str) -> Result<Value, Error> {
        // Take snapshot before evaluation
        self.snapshot_manager.take_snapshot(&self.vm, None);

        // Evaluate
        let result = self.vm.eval(expr)?;

        // Auto-snapshot after each REPL eval
        Ok(result)
    }

    fn handle_time_travel_command(&mut self, cmd: TimeravelCommand) {
        match cmd {
            TimeTravelCommand::Rewind(n) => {
                // Go back n snapshots
                let snapshot_id = self.snapshot_manager.get_nth_from_end(n);
                self.snapshot_manager.restore_snapshot(&mut self.vm, snapshot_id);
            }
            TimeTravelCommand::Replay => {
                // Step forward one snapshot
                // ...
            }
            TimeTravelCommand::Save(name) => {
                self.snapshot_manager.save_named_snapshot(&self.vm, name);
            }
            TimeTravelCommand::Restore(name) => {
                let snapshot_id = self.snapshot_manager.find_named(name)?;
                self.snapshot_manager.restore_snapshot(&mut self.vm, snapshot_id);
            }
        }
    }
}
```

---

### Phase 4: Record & Replay (Advanced, 1-2 weeks)

**rr-style deterministic recording:**

```rust
pub struct ExecutionTrace {
    events: Vec<TraceEvent>,
}

pub enum TraceEvent {
    Eval { expr: String, result: Value },
    Mutate { location: ObjectId, old_value: Value, new_value: Value },
    IORead { source: String, data: Vec<u8> },
    IOWrite { dest: String, data: Vec<u8> },
}

impl VM {
    fn record_eval(&mut self, expr: &str, result: &Value) {
        if self.recording {
            self.trace.events.push(TraceEvent::Eval {
                expr: expr.to_string(),
                result: result.clone(),
            });
        }
    }

    fn replay(&mut self) -> Result<(), Error> {
        for event in &self.trace.events {
            match event {
                TraceEvent::Eval { expr, expected_result } => {
                    let actual_result = self.eval(expr)?;
                    assert_eq!(actual_result, *expected_result);  // Determinism check
                }
                TraceEvent::IORead { source, data } => {
                    // Return recorded data instead of actual I/O
                    self.mock_io_read(source, data);
                }
                _ => {}
            }
        }
        Ok(())
    }
}
```

---

## Performance Characteristics

**Memory overhead:**
```
Snapshot cost: O(1) time, O(k) space where k = changes since last snapshot
History storage: O(n * k) where n = number of snapshots
```

**Typical REPL session:**
- 100 evaluations
- Average 10 mutations per eval
- Snapshot size: ~1KB per eval
- Total history: ~100KB (negligible!)

**Optimization: Garbage collect unreachable snapshots**
```rust
impl SnapshotManager {
    fn gc_snapshots(&mut self) {
        // Keep only reachable snapshots:
        // 1. Named snapshots
        // 2. Last N auto-snapshots
        // 3. Snapshots with debugger breakpoints

        let reachable = self.compute_reachable_snapshots();
        self.snapshots.retain(|id, _| reachable.contains(id));
    }
}
```

---

## Use Cases

### 1. REPL Time-Travel
```scheme
patina> (define lst '(1 2 3))
patina> (set-car! lst 99)
patina> lst
(99 2 3)

patina> (rewind 1)
patina> lst
(1 2 3)
```

### 2. Debugging Crashes
```scheme
patina> (snapshot-save "before-test")
patina> (run-tests)
Error in test-divide: Division by zero

patina> (snapshot-restore "before-test")
patina> (debug run-tests)  ; ← Step through with debugger
```

### 3. Exploring State Space
```scheme
patina> (define game-state (initial-state))
patina> (play-move 'north)
patina> (snapshot-save "went-north")
patina> (play-move 'east)
; Dead end!

patina> (snapshot-restore "went-north")
patina> (play-move 'west)  ; ← Try different path
```

---

## Integration with Debugger

```rust
impl Debugger {
    fn set_watchpoint(&mut self, var: Symbol) {
        // Take snapshot whenever var changes
        self.watch_vars.insert(var);
    }

    fn on_variable_change(&mut self, var: Symbol, old_val: Value, new_val: Value) {
        if self.watch_vars.contains(&var) {
            self.snapshot_manager.take_snapshot(
                &self.vm,
                Some(format!("{} changed: {} -> {}", var, old_val, new_val))
            );
        }
    }

    fn inspect_historical_state(&mut self, snapshot_id: SnapshotId) {
        let snapshot = &self.snapshot_manager.snapshots[&snapshot_id];

        println!("Snapshot: {}", snapshot.description);
        println!("Time: {:?}", snapshot.timestamp);
        println!("Globals:");
        for (name, value) in &snapshot.globals {
            println!("  {} = {}", name, value);
        }
    }
}
```

---

## Challenges

**Challenge 1: I/O Non-Determinism**
- Problem: Can't replay I/O operations deterministically
- Solution: Record I/O during initial execution, replay from recording

**Challenge 2: Memory Overhead**
- Problem: Too many snapshots consume RAM
- Solution: Configurable history limit, GC old snapshots

**Challenge 3: External State**
- Problem: FFI calls, file system changes aren't captured
- Solution: Warn when non-replayable operations occur

---

## References

1. **"rr: Lightweight Recording & Replay"** (O'Callahan et al., 2017)
   - Deterministic record/replay for native code

2. **"Time-Travel Debugging for JavaScript"** (Barr et al., 2016)
   - Similar ideas for dynamic languages

3. **Persistent Data Structures:**
   - `im` crate (Rust): Fast immutable data structures
   - Clojure's persistent collections

---

## Success Metrics

- ✅ Snapshot overhead: <1ms per REPL eval
- ✅ Memory usage: <100MB for 1000 snapshots
- ✅ UX: Users love time-travel debugging

**This feature would be unique to Patina - few Schemes have this!** 🎯
