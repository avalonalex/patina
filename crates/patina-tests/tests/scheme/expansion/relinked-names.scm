;; What a library macro's expansion *relinks*: a template identifier that
;; means a binding the program cannot name — the library's private variable
;; or keyword, a definition an expansion introduced — or a name the program
;; means something else by. Such an identifier is given a spelling of its own
;; in the expansion, and what that spelling is bound to is the subject here.
;;
;; **This file is a scoreboard, landed before the change it scores** (#837's
;; second PR). Every row passed when it was written, on both backends, and
;; says "nothing about this shape changes": the rows are the relinking
;; shapes that no lane held — a generated macro over a private variable or
;; keyword, in the form that made it and in later ones; a relinked
;; assignment; introduced state; a keyword an expansion introduces under a
;; relinked spelling; a definition an expansion produces into a body beside
;; a reference to it; a generated macro's relinked literal; a template that
;; itself writes a name spelled like a relinked one. `hygiene_matrix.rs`
;; scores the first two families and, by its own assertion, none of the
;; rest. The same shapes with a collection or a library redefinition between
;; the steps, which no oracle can stage, are Rust tests
;; (`macro_definition_env.rs`, `gc_shared_tests!`), and the splicing one is a
;; row of `expansion/splicing-libraries.scm`.
;;
;; ── Why this file needs libraries, and what that costs ─────────────────────
;;
;; Relinking applies to a macro from *another* library, so the macros are
;; defined in libraries inline. **chibi 0.12 cannot run this file** — it does
;; not take `define-library` in a script; `expansion/template-references.scm`
;; has the measurement — and is registered `*` / `incomplete` in
;; `DIVERGENCES.tsv`. Gauche 0.9.15 runs it and answers all thirty-eight rows
;; as asserted.
;;
;; Every row was also run under chibi 0.12 with the libraries as `.sld`
;; files and the rest of this file as the program (2026-11-16). It agrees on
;; thirty of the thirty-eight. The eight it does not, each of which says so
;; where it stands:
;;
;; - "a template's assignment writes the library's private variable": chibi
;;   answers `(3 1 program)`: a procedure inside the library reads another
;;   value than the template's expansions wrote, and the template's own read
;;   in the next row agrees again. No claim about why.
;; - Seven of the nine rows on a keyword an expansion introduces under its
;;   library's private spelling. chibi's top-level `define-syntax` lands on
;;   the *library's* keyword, so the library's own macro and every earlier
;;   generated macro expand the newest definition: `((introduced 2)
;;   (introduced 3))`, `((one 4) (one 5))`, `((one 6) (one 7))`, `((two 8)
;;   (two 8) ((one 6) (one 7)))`, `((late 9) (late 9) (late 9))`,
;;   `((from-l3-again 1) (from-l3-again 2) (from-l3-again 3))` and
;;   `((from-l3-again 4) (from-l3-again 5))`, in the file's order. Gauche
;;   keeps each expansion's keyword its own, as Patina does, and it is the
;;   arbiter of these rows.
;;
;; Left out because chibi and Gauche disagree with each other: a generated
;; macro's private literal against the same spelling written by a program
;; that binds nothing of that name (chibi: no match; Gauche: a match).
;;
;; ── Staging ─────────────────────────────────────────────────────────────────
;;
;; The rows share one program, so each group has a library and a private
;; spelling of its own, and where a row depends on what the program has or
;; has not defined when a form is compiled, the comment says so. Three
;; places are ordered on purpose: the collision row is first, before the
;; program defines `secret`; the second definer library is defined and
;; imported only after the first one's rows; and the `else` rows are last,
;; since the program's definition of `else` is the program's for good.

(import (scheme base) (srfi 64))

;; ─── The libraries ──────────────────────────────────────────────────────────

;; The template's own text: forty exported names spelled as a relinked
;; `secret` might be, beside the private `secret`, in one template.
(define-library (probe collide)
  (export collide-all
          secret.0 secret.1 secret.2 secret.3 secret.4 secret.5 secret.6
          secret.7 secret.8 secret.9 secret.10 secret.11 secret.12 secret.13
          secret.14 secret.15 secret.16 secret.17 secret.18 secret.19
          secret.20 secret.21 secret.22 secret.23 secret.24 secret.25
          secret.26 secret.27 secret.28 secret.29 secret.30 secret.31
          secret.32 secret.33 secret.34 secret.35 secret.36 secret.37
          secret.38 secret.39)
  (import (scheme base))
  (begin
    (define secret 42)
    (define secret.0 'real-0) (define secret.1 'real-1)
    (define secret.2 'real-2) (define secret.3 'real-3)
    (define secret.4 'real-4) (define secret.5 'real-5)
    (define secret.6 'real-6) (define secret.7 'real-7)
    (define secret.8 'real-8) (define secret.9 'real-9)
    (define secret.10 'real-10) (define secret.11 'real-11)
    (define secret.12 'real-12) (define secret.13 'real-13)
    (define secret.14 'real-14) (define secret.15 'real-15)
    (define secret.16 'real-16) (define secret.17 'real-17)
    (define secret.18 'real-18) (define secret.19 'real-19)
    (define secret.20 'real-20) (define secret.21 'real-21)
    (define secret.22 'real-22) (define secret.23 'real-23)
    (define secret.24 'real-24) (define secret.25 'real-25)
    (define secret.26 'real-26) (define secret.27 'real-27)
    (define secret.28 'real-28) (define secret.29 'real-29)
    (define secret.30 'real-30) (define secret.31 'real-31)
    (define secret.32 'real-32) (define secret.33 'real-33)
    (define secret.34 'real-34) (define secret.35 'real-35)
    (define secret.36 'real-36) (define secret.37 'real-37)
    (define secret.38 'real-38) (define secret.39 'real-39)
    (define-syntax collide-all
      (syntax-rules ()
        ((_) (list secret
               secret.0 secret.1 secret.2 secret.3 secret.4 secret.5 secret.6
               secret.7 secret.8 secret.9 secret.10 secret.11 secret.12
               secret.13 secret.14 secret.15 secret.16 secret.17 secret.18
               secret.19 secret.20 secret.21 secret.22 secret.23 secret.24
               secret.25 secret.26 secret.27 secret.28 secret.29 secret.30
               secret.31 secret.32 secret.33 secret.34 secret.35 secret.36
               secret.37 secret.38 secret.39))))))

;; #837's shape: a macro that defines a macro in the program whose template
;; names the library's private variable.
(define-library (probe getter)
  (export def-getter-maker bump!)
  (import (scheme base))
  (begin
    (define secret 42)
    (define (bump!) (set! secret (+ secret 1)))
    (define-syntax def-getter-maker
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) secret))))))))

;; The same under another private spelling, which the program defines only
;; after it has compiled uses.
(define-library (probe later)
  (export def-later-getter bump-hidden!)
  (import (scheme base))
  (begin
    (define hidden 42)
    (define (bump-hidden!) (set! hidden (+ hidden 1)))
    (define-syntax def-later-getter
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) hidden))))))))

;; Templates that call procedures the program imports too, and then defines.
(define-library (probe casing)
  (export up-of down-of)
  (import (scheme base) (scheme char))
  (begin
    (define-syntax up-of (syntax-rules () ((_ c) (char-upcase c))))
    (define-syntax down-of (syntax-rules () ((_ c) (char-downcase c))))))

(define-library (probe mint)
  (export call-f f call-h h)
  (import (scheme base))
  (begin
    (define (f) 'lib-f)
    (define (h) 'lib-h)
    (define-syntax call-f (syntax-rules () ((_) (f))))
    (define-syntax call-h (syntax-rules () ((_) (h))))))

;; A template that assigns the library's private variable.
(define-library (probe setter)
  (export bump-secret! read-secret lib-read)
  (import (scheme base))
  (begin
    (define secret 0)
    (define (lib-read) secret)
    (define-syntax read-secret (syntax-rules () ((_) secret)))
    (define-syntax bump-secret! (syntax-rules () ((_) (set! secret (+ secret 1)))))))

;; And a generated macro whose template assigns one.
(define-library (probe genset)
  (export gen-set bump-nested!)
  (import (scheme base))
  (begin
    (define secret 42)
    (define (bump-nested!) (set! secret (+ secret 1)))
    (define-syntax gen-set
      (syntax-rules ()
        ((_ name)
         (define-syntax name
           (syntax-rules () ((_ v) (begin (set! secret v) secret)))))))))

;; A definer of introduced state: each use introduces a `count` of its own,
;; which only its scopes select, beside the library's own `count`.
(define-library (probe intro)
  (export get1 bump1! get2 bump2! plain-count def-counter)
  (import (scheme base))
  (begin
    (define count 100)
    (define (plain-count) count)
    (define-syntax def-counter
      (syntax-rules ()
        ((_ getter bumper)
         (begin
           (define count 0)
           (define-syntax getter (syntax-rules () ((_) count)))
           (define-syntax bumper (syntax-rules () ((_) (set! count (+ count 1)))))))))
    (def-counter get1 bump1!)
    (def-counter get2 bump2!)))

;; One template reads the private spelling free; another binds it around its
;; argument.
(define-library (probe shadow)
  (export m tag-of)
  (import (scheme base))
  (begin
    (define tmp 'lib)
    (define-syntax m (syntax-rules () ((_ x) (list tmp x))))
    (define-syntax tag-of
      (syntax-rules () ((_ x) (let ((tmp 'local)) (list tmp x)))))))

;; Generated macros over the library's private keyword, one and two levels.
(define-library (probe kwgen)
  (export def-user def-user2 direct)
  (import (scheme base))
  (begin
    (define-syntax label (syntax-rules () ((_ x) (list 'lib-label x))))
    (define-syntax direct (syntax-rules () ((_ x) (label x))))
    (define-syntax def-user
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_ x) (label x)))))))
    (define-syntax def-user2
      (syntax-rules ()
        ((_ mid)
         (define-syntax mid
           (syntax-rules ()
             ((_ name)
              (define-syntax name (syntax-rules () ((_ x) (label x))))))))))))

;; Templates that *introduce a keyword* spelled as the library's private
;; keyword `priv`, or as its private variable `kw-secret`, at top level.
(define-library (probe kwintro)
  (export m-kw def-kw def-kw-late use-priv def-kw-var use-kw-secret)
  (import (scheme base))
  (begin
    (define-syntax priv (syntax-rules () ((_ x) (list 'lib-priv x))))
    (define-syntax use-priv (syntax-rules () ((_ x) (priv x))))
    ;; The keyword and its use in one expansion.
    (define-syntax m-kw
      (syntax-rules ()
        ((_ v x)
         (begin (define-syntax priv (syntax-rules () ((_ y) (list 'introduced y))))
                (define v (priv x))))))
    ;; Its user is a generated macro, used in later forms.
    (define-syntax def-kw
      (syntax-rules ()
        ((_ name tag)
         (begin (define-syntax priv (syntax-rules () ((_ y) (list 'tag y))))
                (define-syntax name (syntax-rules () ((_ y) (priv y))))))))
    ;; The generated macro is compiled before the keyword it uses is defined.
    (define-syntax def-kw-late
      (syntax-rules ()
        ((_ name tag)
         (begin (define-syntax name (syntax-rules () ((_ y) (priv y))))
                (define-syntax priv (syntax-rules () ((_ y) (list 'tag y))))))))
    ;; A private *variable's* spelling introduced as a keyword.
    (define kw-secret 42)
    (define-syntax use-kw-secret (syntax-rules () ((_) kw-secret)))
    (define-syntax def-kw-var
      (syntax-rules ()
        ((_ name)
         (begin (define-syntax kw-secret (syntax-rules () ((_) 'introduced-kw)))
                (define-syntax name (syntax-rules () ((_) (kw-secret))))))))))

;; A definition of the private spelling beside a reference to it.
(define-library (probe settle)
  (export beside get-settled)
  (import (scheme base))
  (begin
    (define settled 42)
    (define-syntax get-settled (syntax-rules () ((_) settled)))
    (define-syntax beside
      (syntax-rules ()
        ((_ v) (begin (define settled 'introduced) (define v settled)))))))

;; Mutually recursive definitions and their use as one expansion, the library
;; having private procedures of both spellings.
(define-library (probe fwd)
  (export m-produced)
  (import (scheme base))
  (begin
    (define (ev? n) 'lib-ev)
    (define (od? n) 'lib-od)
    (define-syntax m-produced
      (syntax-rules ()
        ((_ x)
         (begin
           (define (ev? n) (if (= n 0) 'even (od? (- n 1))))
           (define (od? n) (if (= n 0) 'odd (ev? (- n 1))))
           (ev? x)))))))

;; Generated macros with a literal: the library's private variable `lit`,
;; its private variable `priv-lit` (which the program binds nothing of), and
;; `else`.
(define-library (probe litguard)
  (export def-lit-matcher use-lit)
  (import (scheme base))
  (begin
    (define lit 'lib-lit)
    (define-syntax def-lit-matcher
      (syntax-rules ()
        ((_ name)
         (define-syntax name
           (syntax-rules (lit) ((_ lit) 'matched) ((_ x) 'other))))))
    (define-syntax use-lit (syntax-rules () ((_ g) (g lit))))))

(define-library (probe litonly)
  (export def-priv-matcher feed)
  (import (scheme base))
  (begin
    (define priv-lit 'the-library-binding)
    (define-syntax def-priv-matcher
      (syntax-rules ()
        ((_ name)
         (define-syntax name
           (syntax-rules (priv-lit)
             ((_ priv-lit) 'matched-the-literal)
             ((_ anything) 'no-match))))))
    (define-syntax feed (syntax-rules () ((_ k) (k priv-lit))))))

(define-library (probe litgen)
  (export def-else-matcher use-else via-cond)
  (import (scheme base))
  (begin
    (define-syntax def-else-matcher
      (syntax-rules ()
        ((_ name)
         (define-syntax name
           (syntax-rules (else) ((_ else) 'lit) ((_ x) 'other))))))
    (define-syntax use-else (syntax-rules () ((_ g) (g else))))
    (define-syntax via-cond (syntax-rules () ((_ x) (cond (#f 1) (else x)))))))

(import (probe collide) (probe getter) (probe later) (probe casing)
        (probe mint) (probe setter) (probe genset) (probe intro)
        (probe shadow) (probe kwgen) (probe kwintro) (probe settle) (probe fwd) (probe litguard) (probe litonly)
        (probe litgen))

(test-begin "relinked-names")

;; ─── The template's own text ────────────────────────────────────────────────

;; First in the file, while the program binds no `secret`. The private
;; `secret` reads the library's 42, and every exported `secret.k` its own
;; value: the second element lists the ones that did not.
(define collided (collide-all))

(test-equal "a template's own name spelled like a relinked one reads its own value"
  '(42 ())
  (list (car collided)
        (let loop ((k 0) (xs (cdr collided)) (bad '()))
          (if (null? xs)
              (reverse bad)
              (loop (+ k 1)
                    (cdr xs)
                    (if (eq? (car xs)
                             (string->symbol
                              (string-append "real-" (number->string k))))
                        bad
                        (cons (cons k (car xs)) bad)))))))

;; ─── A generated macro over a private variable ──────────────────────────────

(def-getter-maker get)
(define secret 'program)
(bump!)

(test-equal "a generated macro reads the library's private variable past the program's"
  '(43 program)
  (list (get) secret))

(define (get-later) (get))
(bump!)

(test-equal "and in a procedure compiled later, and under a local binding of the spelling"
  '(44 44)
  (list (get-later) (let ((secret 'local)) (get))))

;; The uses come first here, and the program's definition of the spelling
;; after them.
(def-later-getter get-hidden)
(define (use-in-proc) (list (get-hidden) (get-hidden)))

(test-equal "a relinked reference compiled into a procedure reads the library's"
  '(42 42)
  (use-in-proc))

(bump-hidden!)
(define-syntax wrap (syntax-rules () ((_) (list 'wrapped (get-hidden)))))
(define hidden 'program)

(test-equal "a later definition of the spelling moves neither earlier code nor later expansions"
  '((43 43) (wrapped 43) (wrapped 43))
  (list (use-in-proc) (wrap) (let ((hidden 1)) (wrap))))

(def-later-getter get-hidden-again)
(bump-hidden!)

(test-equal "a second run of the generator names the same private variable"
  '(44 44)
  (list (get-hidden) (get-hidden-again)))

;; ─── A reference in the form that defines its spelling ──────────────────────

;; The program defines a name it imported, and expands a template that
;; means the import, in one form. The template's is the library's.
(begin
  (define (char-upcase c) 'mine)
  (test-equal "a template reference beside the program's definition of the spelling, in one form"
    '(#\A mine)
    (list (up-of #\a) (char-upcase 0))))

(begin
  (define (f) 'program-f)
  (test-equal "and of a name the template's library exports"
    '(lib-f program-f)
    (list (call-f) (f))))

(test-equal "and in the forms after"
  '(#\B mine lib-f program-f)
  (list (up-of #\b) (char-upcase 0) (call-f) (f)))

;; The same definitions in forms of their own, between two procedures that
;; expand the templates.
(define (before) (list (call-h) (down-of #\A)))
(define (h) 'program-h)
(define (char-downcase c) 'program-downcase)
(define (after) (list (call-h) (down-of #\A)))

(test-equal "a definition over an import captures no template reference, before or after"
  '((lib-h #\a) (lib-h #\a) program-h program-downcase)
  (list (before) (after) (h) (char-downcase 0)))

;; ─── A relinked assignment ──────────────────────────────────────────────────

(bump-secret!)
(define (twice) (bump-secret!) (bump-secret!) (read-secret))

;; chibi 0.12 answers `(3 1 program)`: the header has it.
(test-equal "a template's assignment writes the library's private variable"
  '(3 3 program)
  (list (twice) (lib-read) secret))

(test-equal "and under a local binding of the spelling"
  '(local 4)
  (let ((secret 'local)) (bump-secret!) (list secret (read-secret))))

(gen-set put!)

(test-equal "a generated macro's assignment writes it too"
  '(7 program)
  (list (put! 7) secret))

(bump-nested!)
(define (put-later v) (put! v))

(test-equal "and in a procedure compiled later"
  '(9 program)
  (list (put-later 9) secret))

;; ─── Introduced definitions ─────────────────────────────────────────────────

(define count 'program)
(bump1!) (bump1!) (bump2!)

(test-equal "each run of a definer has its own introduced state"
  '(2 1 100 program)
  (list (get1) (get2) (plain-count) count))

(define (bump-later) (bump2!) (list (get1) (get2)))

(test-equal "which a procedure compiled later reaches"
  '(2 2)
  (bump-later))

(def-counter get3 bump3!)
(bump3!)

(test-equal "and a run in the program has another"
  '(2 2 1 100 program)
  (list (get1) (get2) (get3) (plain-count) count))

;; ─── A template's own binder of the spelling ────────────────────────────────

(define tmp 'program)

(test-equal "a free mention and a template's own binder of one spelling keep apart"
  '((lib 0) (lib (lib 0)) (local program) (local (lib 1)) (lib user-local))
  (list (m 0) (m (m 0)) (tag-of tmp) (tag-of (m 1))
        (let ((tmp 'user-local)) (m tmp))))

;; ─── A generated macro over a private keyword ───────────────────────────────

(def-user use)
(define-syntax label (syntax-rules () ((_ x) (list 'program-label x))))

(test-equal "a generated macro expands the library's private keyword past the program's"
  '((lib-label 1) (lib-label 2) (program-label 3))
  (list (use 1) (direct 2) (label 3)))

(define (use-later)
  (let-syntax ((label (syntax-rules () ((_ x) 'local))))
    (list (use 4) (direct 5) (label 6))))

(test-equal "and under a let-syntax of the spelling, in a procedure"
  '((lib-label 4) (lib-label 5) local)
  (use-later))

(def-user2 mid)
(mid use2)

(test-equal "and through two levels of generated macro, at top level and in a body"
  '((lib-label 7) (lib-label 8))
  (list (use2 7) (let () (mid local-use) (local-use 8))))

;; ─── An introduced keyword of a relinked spelling ───────────────────────────

;; Each template below defines a keyword spelled as its library's private
;; `priv` at top level, and the keyword is the expansion's own: `use-priv`,
;; the library's macro over its own `priv`, answers `lib-priv` throughout.
;; Gauche 0.9.15 answers every row as asserted. chibi 0.12's top-level
;; `define-syntax` lands on the library's own keyword, so it differs on five
;; of these seven rows and on the two cross-library rows: see the header.

(test-equal "the library's own keyword, before any definer has run"
  '(lib-priv 1)
  (use-priv 1))

(m-kw v1 2)

(test-equal "a keyword an expansion introduces under a relinked spelling is its own"
  '((introduced 2) (lib-priv 3))
  (list v1 (use-priv 3)))

(def-kw g one)

(test-equal "a generated macro uses the keyword its definer introduced"
  '((one 4) (lib-priv 5))
  (list (g 4) (use-priv 5)))

(define (in-proc) (list (g 6) (use-priv 7)))

(test-equal "and in a procedure compiled after the definer ran"
  '((one 6) (lib-priv 7))
  (in-proc))

(def-kw g2 two)

(test-equal "a second run of the definer introduces a second keyword"
  '((one 8) (two 8) ((one 6) (lib-priv 7)))
  (list (g 8) (g2 8) (in-proc)))

(def-kw-late gl late)

(test-equal "a generated macro compiled before the keyword it uses is defined"
  '((late 9) (one 9) (two 9))
  (list (gl 9) (g 9) (g2 9)))

(def-kw-var gv)

(test-equal "a private variable's spelling introduced as a keyword"
  '(introduced-kw 42)
  (list (gv) (use-kw-secret)))

;; A second library runs the first one's definer and exports what it made.
;; Defined and imported here, after the rows above, so that under chibi,
;; which loads a library when it is imported and whose `define-syntax` lands
;; on the first library's keyword, those rows are measured without it.
(define-library (probe kwintro3)
  (export g3 g3b)
  (import (scheme base) (probe kwintro))
  (begin
    (def-kw g3 from-l3)
    (def-kw g3b from-l3-again)))

(import (probe kwintro3))

(test-equal "a definer run in a second library, whose macros a third root uses"
  '((from-l3 1) (from-l3-again 2) (lib-priv 3))
  (list (g3 1) (g3b 2) (use-priv 3)))

(define (cross-in-proc) (list (g3 4) (g3b 5)))

(test-equal "and in a procedure of that root"
  '((from-l3 4) (from-l3-again 5))
  (cross-in-proc))

;; ─── A definition an expansion produces beside a reference ──────────────────

;; Into a body: when the body is desugared nothing selects the reference to
;; the definition its own expansion produced, and it reads that definition
;; all the same, where `get-settled` reads the library's.
(define (settle-in-body) (beside w) (list w (get-settled)))

(test-equal "a reference beside a definition its expansion produces into a body"
  '(introduced 42)
  (settle-in-body))

;; At top level the definition is declared at its scopes before the
;; reference is resolved.
(beside v-top)

(test-equal "and at top level"
  '(introduced 42)
  (list v-top (get-settled)))

(define (fwd-in-proc) (m-produced 4))

(test-equal "a forward reference to a definition the expansion produces into a body"
  '(odd even)
  (list (let () (m-produced 5)) (fwd-in-proc)))

(test-equal "and into a lambda body"
  'odd
  ((lambda (k) (m-produced k)) 3))

;; ─── A generated macro's relinked literal ───────────────────────────────────

;; The program's `lit` makes the library's a relinked literal of `lit-g`.
;; The library's own mention matches it; the program's `lit` is another
;; binding, and `zzz` no binding at all.
(define lit 'program-lit)
(def-lit-matcher lit-g)

(test-equal "a relinked literal matches the library's own mention and nothing of the program's"
  '(matched other other)
  (list (use-lit lit-g) (lit-g lit) (lit-g zzz)))

;; A literal the generated macro mentions in no template, of a name the
;; program binds nothing of.
(def-priv-matcher priv-g)

(test-equal "a literal-only generated macro matches its library's mention"
  'matched-the-literal
  (feed priv-g))

;; Last in the file: the program defines `else`. The generated macro's
;; literal is the library's `else`, which is `cond`'s; the program's is not.
(define else #f)
(def-else-matcher else-g)

(test-equal "a generated macro's else is the library's, not the program's"
  '(other lit 7)
  (list (else-g else) (use-else else-g) (via-cond 7)))

(define (else-in-proc) (list (else-g else) (use-else else-g) (via-cond 8)))

(test-equal "and in a procedure, the else rows again"
  '(other lit 8)
  (else-in-proc))

(test-end)
