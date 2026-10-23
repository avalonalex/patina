;; records.scm -- Record types for (scheme base)
;;
;; Record types (R7RS Section 5.5)
;;
;; define-record-type creates a new record type with:
;; - A type descriptor (bound to <name>)
;; - A constructor procedure
;; - A predicate procedure
;; - Accessor and optional mutator procedures for each field
;;
;; The constructor takes a formal for each field it names, so the machine
;; checks how many arguments it is given, and it makes the record in one
;; call. The accessors, mutators and predicate check a record's type in
;; Rust, comparing its descriptor with theirs by identity, with no
;; allocation (#736). An accessor or mutator captures one key, its type and
;; its field's index paired, so that a call runs as many of the VM's
;; instructions as an unchecked one did.
;;
;; No lambda of the expansion has a formal of the program's, so a field may
;; be named as its type, or as a variable of the expansion. The templates
;; bind by applying a lambda, which is what `let` expands to, since each
;; `let` costs a step of expansion: as `let`s, they were a third of what
;; defining a record type of two fields cost. The two helpers below are not
;; exported: the template resolves them where they are defined, as `guard`'s
;; does `%guard-aux`.

;; The indices of the fields named `fields` in the record type `rtd`, as a
;; vector: a constructor's, computed once when it is defined (#736).
(define (%record-field-indices rtd fields)
  (list->vector
   (map (lambda (field) (%record-type-field-index rtd field)) fields)))

;; A constructor of the record type `type` with a formal for each field it
;; is given, which `indices` places. The formals are this macro's own rather
;; than the fields: with the fields as formals, a field named as a variable
;; of the expansion, as srfi-19's `type` is, made that variable's references
;; ambiguous (#771). A step makes up to four, so a constructor of four
;; fields or fewer takes one step, and each step's formals are new names.
(define-syntax %record-constructor
  (syntax-rules ()
    ((_ type indices () (formal ...))
     (lambda (formal ...)
       (%make-record-from type indices formal ...)))
    ((_ type indices (f1) (formal ...))
     (lambda (formal ... v1)
       (%make-record-from type indices formal ... v1)))
    ((_ type indices (f1 f2) (formal ...))
     (lambda (formal ... v1 v2)
       (%make-record-from type indices formal ... v1 v2)))
    ((_ type indices (f1 f2 f3) (formal ...))
     (lambda (formal ... v1 v2 v3)
       (%make-record-from type indices formal ... v1 v2 v3)))
    ((_ type indices (f1 f2 f3 f4) (formal ...))
     (lambda (formal ... v1 v2 v3 v4)
       (%make-record-from type indices formal ... v1 v2 v3 v4)))
    ((_ type indices (f1 f2 f3 f4 . fields) (formal ...))
     (%record-constructor type indices fields (formal ... v1 v2 v3 v4)))))

;; Helper macro to define accessor and optional mutator for a field
(define-syntax %define-field-accessors
  (syntax-rules ()
    ;; Field with accessor only
    ((%define-field-accessors rtd field-name accessor)
     (define accessor
       ((lambda (field)
          (lambda (record)
            (%record-field record field)))
        (cons rtd (%record-type-field-index rtd 'field-name)))))
    ;; Field with accessor and mutator
    ((%define-field-accessors rtd field-name accessor mutator)
     (begin
       (define accessor
         ((lambda (field)
            (lambda (record)
              (%record-field record field)))
          (cons rtd (%record-type-field-index rtd 'field-name))))
       (define mutator
         ((lambda (field)
            (lambda (record value)
              (%record-field-set! record field value)))
          (cons rtd (%record-type-field-index rtd 'field-name))))))))

;; Main define-record-type macro
;; Syntax: (define-record-type <name>
;;           (<constructor> <field-name> ...)
;;           <pred>
;;           (<field-name> <accessor>) ...
;;           (<field-name> <accessor> <mutator>) ...)
(define-syntax define-record-type
  (syntax-rules ()
    ((define-record-type name
       (constructor constructor-field ...)
       pred
       (field-name accessor . maybe-mutator) ...)
     (begin
       ;; Create the record type descriptor
       (define name (%make-record-type 'name '(field-name ...)))

       ;; Create the predicate
       (define pred
         ((lambda (type)
            (lambda (obj)
              (%record-of? obj type)))
          name))

       ;; Create the constructor: the fields it names, in its order, at
       ;; their indices; the others unspecified
       (define constructor
         ((lambda (type indices)
            (%record-constructor type indices (constructor-field ...) ()))
          name (%record-field-indices name '(constructor-field ...))))

       ;; Create accessors and mutators for each field
       (%define-field-accessors name field-name accessor . maybe-mutator)
       ...))))
