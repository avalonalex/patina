# Interactive Tutorial Design: `patina learn`

## Summary

A built-in conversational Scheme tutorial in the style of *The Little Schemer*, powered by Claude. The tutor guides users through Scheme concepts progressively, asks questions, evaluates their answers in the real Patina runtime, and explains results. Strictly scoped to teaching Scheme — not a general-purpose chatbot.

## Core Interaction Model

```
$ patina learn

Welcome to Patina Learn — an interactive Scheme tutorial.
(Type "quit" to exit, "help" for commands, "skip" to skip ahead)

─── Chapter 1: Atoms and Lists ───

What is the value of (+ 1 2)?
> 3

Correct! The + procedure adds numbers together.
Numbers like 1, 2, and 3 are called atoms — they're indivisible values.

Now try: what is (car '(a b c))?
> (car '(a b c))
=> a

Right! car returns the first element of a list.
And what does cdr return?
> the rest of the list
Yes — cdr returns everything after the first element.

What is (cdr '(a b c))?
> (b c)

Correct! (cdr '(a b c)) => (b c).

What happens if you try (car '())?
> (car '())
Error: car: contract violation — expected pair, got ()

That error is expected. car is only defined on pairs (non-empty lists).
This is an important principle: always check for the empty list
before calling car or cdr. We'll see how with cond and null? soon.
```

### Input Modes

The tutor handles three kinds of input:

1. **Scheme expressions** — detected by leading `(`, `'`, `#`, or if it parses as valid Scheme. Evaluated in the Patina runtime, result shown, then discussed by the tutor.

2. **Direct answers** — short text like `3`, `a`, `#t`, `(b c)`. Compared against the expected answer. The tutor confirms or corrects.

3. **Questions in English** — anything else. Sent to Claude with the strict Scheme-teaching system prompt. The tutor answers only if it's about Scheme; otherwise deflects politely.

## Curriculum

Progressive chapters, each building on the previous. Inspired by *The Little Schemer* and *SICP* but adapted for R7RS.

### Part I: Foundations

| Ch | Topic | Key Concepts |
|----|-------|-------------|
| 1 | Atoms and Lists | Numbers, strings, booleans, symbols, `quote`, `'(...)` |
| 2 | car, cdr, cons | List access, construction, pairs vs lists |
| 3 | Predicates | `null?`, `pair?`, `eq?`, `equal?`, `number?`, `zero?` |
| 4 | Conditionals | `if`, `cond`, `and`, `or`, truthiness |
| 5 | Defining things | `define`, `let`, `let*`, scope |
| 6 | Lambda | Anonymous functions, closures, first-class procedures |

### Part II: Recursion

| Ch | Topic | Key Concepts |
|----|-------|-------------|
| 7 | Simple recursion | `length`, `append`, `member`, base case + recursive case |
| 8 | Recursion on numbers | `factorial`, `fibonacci`, accumulator pattern |
| 9 | Deep recursion | `flatten`, `count-atoms`, recursion on nested lists |
| 10 | Tail recursion | Named let, tail position, `do` loops |

### Part III: Higher-Order Thinking

| Ch | Topic | Key Concepts |
|----|-------|-------------|
| 11 | map and filter | Higher-order functions, passing functions as arguments |
| 12 | fold and reduce | `fold-left`, `fold-right`, building abstractions |
| 13 | Returning functions | Currying, function factories, closures over mutable state |
| 14 | apply and eval | Applying argument lists, the evaluator |

### Part IV: The Scheme Way

| Ch | Topic | Key Concepts |
|----|-------|-------------|
| 15 | Strings and characters | String operations, `string->list`, `char-alphabetic?` |
| 16 | Vectors and bytevectors | Mutable arrays, `vector-ref`, `vector-set!` |
| 17 | Input and output | `read`, `write`, `display`, ports, files |
| 18 | Macros | `define-syntax`, `syntax-rules`, pattern matching, hygiene |
| 19 | Continuations | `call/cc`, non-local exit, coroutines |
| 20 | Libraries | `define-library`, `import`, `export`, organizing code |

### Exercises

Each chapter ends with 3–5 exercises of increasing difficulty:

```
─── Exercises ───

1. Write (my-length lst) that returns the length of a list.
   (my-length '(a b c)) should return 3.

2. Write (my-append lst1 lst2) that concatenates two lists.
   (my-append '(a b) '(c d)) should return (a b c d).

3. Challenge: Write (my-reverse lst) without using the built-in reverse.

Type your solution, and I'll test it for you.
> (define (my-length lst)
    (if (null? lst) 0
        (+ 1 (my-length (cdr lst)))))

Let me test that...
  (my-length '())     => 0  ✓
  (my-length '(a))    => 1  ✓
  (my-length '(a b c)) => 3  ✓

All tests pass! Well done.
```

The tutor generates test cases and runs them in the Patina runtime.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                patina learn                      │
│                                                  │
│  ┌───────────┐  ┌───────────┐  ┌─────────────┐  │
│  │ Curriculum │  │  Tutor    │  │   Patina    │  │
│  │  Engine    │──│  (Claude) │──│   Runtime   │  │
│  │           │  │           │  │   (eval)    │  │
│  └───────────┘  └───────────┘  └─────────────┘  │
│        │              │              │            │
│        ▼              ▼              ▼            │
│  chapter state   conversation    REPL env        │
│  + progress      history         + definitions   │
└─────────────────────────────────────────────────┘
```

### Components

**Curriculum Engine** — Manages chapter progression, tracks which topics are covered, selects next questions, stores exercise test cases. This is deterministic — no LLM needed for the curriculum skeleton.

**Tutor (Claude API)** — Provides conversational flexibility on top of the structured curriculum. Explains errors, answers follow-up questions, generates hints, evaluates free-form answers. Strictly scoped via system prompt.

**Patina Runtime** — A real interpreter instance. User expressions are evaluated here. Exercise solutions are tested here. The tutor can ask the runtime to evaluate expressions to verify answers.

### Data Flow

```
User types input
  │
  ├─ Looks like Scheme? ──→ Evaluate in Patina runtime
  │                              │
  │                              ├─ Success: show result, send to tutor for commentary
  │                              └─ Error: show error, send to tutor for explanation
  │
  ├─ Short answer? ──→ Compare with expected, send to tutor for feedback
  │
  └─ English text? ──→ Send to tutor (Scheme-only system prompt)
                            │
                            └─ Response displayed to user
```

## Strict Scope Enforcement

The tutor must only teach Scheme. This is enforced at multiple levels.

### System Prompt

```
You are a Scheme programming tutor built into the Patina Scheme interpreter.

STRICT RULES:
1. You ONLY discuss Scheme programming, R7RS, and functional programming concepts.
2. You NEVER generate code in any language other than Scheme.
3. You NEVER help with tasks unrelated to learning Scheme.
4. If the user asks about anything else, respond:
   "I'm here to help you learn Scheme! Do you have a question about
   the current topic, or would you like to move on?"
5. You NEVER execute shell commands, access files, or perform actions
   outside of discussing Scheme code.
6. Keep responses concise — 1-3 paragraphs max. Show, don't tell.
   Prefer showing a short example over a long explanation.
7. Match the user's level. If they struggle with car/cdr, don't
   mention continuations. If they're advanced, skip the basics.
8. When the user writes code, evaluate it mentally and discuss the
   result. The runtime will actually execute it — you discuss the
   output.

CURRENT STATE:
- Chapter: {chapter_number} — {chapter_title}
- Topic: {current_topic}
- The user has completed: {completed_topics}
- Recent conversation: {last_few_exchanges}
```

### Client-Side Guardrails

1. **Input filtering** — Before sending to Claude, strip anything that looks like prompt injection attempts. If input contains "ignore previous instructions", "system prompt", or similar patterns, don't send it — handle locally with a deflection.

2. **Output filtering** — If Claude's response contains non-Scheme code blocks (Python, JavaScript, shell, etc.), strip them and show only the Scheme parts.

3. **Topic drift detection** — Track how many consecutive exchanges are off-topic. After 2 deflections, gently redirect: "Let's get back to {current_topic}. Here's the next question..."

4. **No tool use** — The Claude API call uses no tools. The tutor is text-only. All Scheme evaluation happens locally in Patina, not via Claude.

## API Key Configuration

### Setup

```bash
# Option 1: Environment variable (recommended)
export ANTHROPIC_API_KEY=sk-ant-...

# Option 2: Config file
echo 'sk-ant-...' > ~/.patina/api-key
chmod 600 ~/.patina/api-key

# Option 3: Interactive on first run
$ patina learn
No API key found. To use the interactive tutor, you need a Claude API key.
Get one at: https://console.anthropic.com/

Enter your API key: sk-ant-...
Save to ~/.patina/config? [Y/n] y
Saved. Starting tutorial...
```

### Key Lookup Order

1. `ANTHROPIC_API_KEY` environment variable
2. `PATINA_CLAUDE_API_KEY` environment variable
3. `~/.patina/api-key` file
4. Interactive prompt (first-run only)

### Offline Mode

If no API key is available, fall back to a static tutorial mode:

```
$ patina learn
No API key found. Running in offline mode.
(Offline mode follows a fixed curriculum without conversational AI.)
(Set ANTHROPIC_API_KEY for the full interactive experience.)

─── Chapter 1: Atoms and Lists ───

Scheme has a few basic types of values called "atoms":
  Numbers:  42, 3.14, 1/3
  Strings:  "hello"
  Booleans: #t, #f
  Symbols:  'foo, 'hello

Try evaluating some atoms:
> 42
=> 42
```

Offline mode uses the curriculum engine only — predetermined questions, pattern-matched answers, no LLM. Less flexible but still useful.

## Progress Tracking

### State File: `~/.patina/learn-progress.scm`

```scheme
(learn-progress
  (current-chapter 7)
  (current-section 2)
  (completed-chapters (1 2 3 4 5 6))
  (completed-exercises
    ((chapter 1 exercises (1 2 3 4))
     (chapter 2 exercises (1 2 3))
     (chapter 3 exercises (1 2))
     (chapter 4 exercises (1 2 3 4 5))
     (chapter 5 exercises (1 2 3))
     (chapter 6 exercises (1 2 3 4))))
  (last-session "2026-03-15T14:30:00Z")
  (total-time-minutes 240))
```

### Resume

```
$ patina learn

Welcome back! You were on Chapter 7: Simple Recursion.
Last time, we were writing a function to count list elements.
Ready to continue? [Y/n]
```

## CLI Commands Within the Tutorial

```
help          Show available commands
quit          Exit the tutorial
skip          Skip to next section
chapter N     Jump to chapter N
exercises     Show exercises for current chapter
progress      Show completion status
reset         Reset progress (with confirmation)
hint          Get a hint for the current question/exercise
eval EXPR     Force-evaluate a Scheme expression
```

## Cost Management

### Model Selection

Use `claude-haiku` for tutorial interactions — fast, cheap, good enough for teaching. The tutor doesn't need Opus-level reasoning.

| Interaction | Model | Estimated tokens |
|---|---|---|
| Answer feedback | Haiku | ~200 in, ~100 out |
| Error explanation | Haiku | ~300 in, ~200 out |
| Exercise evaluation | Haiku | ~400 in, ~200 out |
| Conceptual question | Haiku | ~300 in, ~300 out |

**Estimated cost per chapter:** ~$0.01–0.02 (with Haiku pricing).
**Full curriculum (20 chapters):** ~$0.20–0.40 total.

### Token Budget

- Cap conversation history at ~4000 tokens (rolling window)
- Include curriculum context (~500 tokens) in every request
- Maximum response length: 500 tokens (keeps answers concise)
- If approaching rate limits, degrade gracefully to offline mode

## Implementation Phases

### Phase 1: Offline tutorial (1–2 weeks)

- Curriculum engine with chapters 1–6
- Fixed questions with pattern-matched answers
- REPL integration (evaluate user Scheme expressions)
- Progress tracking
- No Claude API needed

**This is useful on its own** — a structured, interactive tutorial built into the interpreter.

### Phase 2: Claude integration (1 week)

- API key setup and management
- System prompt with scope enforcement
- Conversational responses for answer feedback and error explanation
- Input/output filtering

### Phase 3: Exercises with auto-testing (1 week)

- Exercise test case runner
- Tutor generates hints
- Solution evaluation in Patina runtime
- Tracks completed exercises

### Phase 4: Full curriculum (2–3 weeks)

- Chapters 7–20
- Adaptive difficulty (skip ahead if user is advanced)
- Topic cross-references ("Remember when we learned about car in Chapter 2?")

### Phase 5: Polish (1 week)

- Colored terminal output (chapter headers, correct/incorrect, code highlighting)
- Timing and statistics
- `patina learn --chapter 12` for direct chapter access
- `patina learn --exercise 7.3` for specific exercises

## Open Questions

1. **Curriculum as data or code?** Should chapters be hardcoded Rust structs, or loaded from `.scm` files in `lib/patina/learn/`? Data files are easier to edit and contribute to.

2. **Community contributions?** Could users submit new chapters/exercises as `.scm` files? A curriculum format would enable this.

3. **Multiple tracks?** Beyond the linear progression — a "data structures" track, a "macros deep dive" track, a "SICP companion" track?

4. **Localization?** The curriculum is English-only initially. Claude can respond in the user's language, but the structured parts (chapter titles, exercise descriptions) would need translation.

5. **Integration with `patina pkg`?** If a user is learning about libraries (Chapter 20), could the tutorial walk them through `patina pkg add` to install a real library?
