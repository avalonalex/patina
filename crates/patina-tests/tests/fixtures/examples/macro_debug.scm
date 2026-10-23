;; Macro Debug Tracing Example
;; ===========================
;; This file demonstrates how to use debug tracing to observe macro expansion.
;;
;; Usage: Paste these expressions into the REPL to see macro expansion in action.

;; First, define a simple macro
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

;; Enable macro expansion debug tracing
(macro-debug-mode 'on)

;; Now watch the macro expansion happen!
;; The debug output will show:
;; - The original macro call
;; - The expanded form after substitution

;; Example 1: Simple expansion
(when #t 42)
;; Debug output shows, among its lines:
;; [MACRO] Expanding macro: when
;; [MACRO] Result: (if #t (begin 42))
;; Result: 42

;; Example 2: Multiple body forms
(when #t 1 2 3)
;; Debug output shows how multiple body forms are wrapped in begin
;; Result: 3

;; Example 3: False condition
(when #f 99)
;; Shows the same expansion, but if evaluates to #<unspecified>
;; Result: #<unspecified>

;; Disable debug tracing
(macro-debug-mode 'off)

;; Now this won't show debug output
(when #t 100)
;; Result: 100 (no debug output)

;; You can also ask whether tracing is on
(macro-debug-mode 'status)
;; Result: disabled
