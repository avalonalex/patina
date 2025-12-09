;; records.scm -- Record types for (scheme base)
;;
;; Record types (R7RS Section 5.5)
;;
;; define-record-type creates a new record type with:
;; - A type descriptor (bound to <name>)
;; - A constructor procedure
;; - A predicate procedure
;; - Accessor and optional mutator procedures for each field

;; Helper to create a constructor that maps constructor args to field positions
;; rtd: record type descriptor
;; constructor-fields: list of field names used in constructor (in constructor order)
;; all-fields: list of all field names (in declaration order)
(define (%make-constructor rtd constructor-fields all-fields)
  (let ((num-fields (length all-fields))
        (field-indices
         (map (lambda (cf)
                (%record-type-field-index rtd cf))
              constructor-fields)))
    (lambda args
      (let ((field-vec (make-vector num-fields)))
        ;; Initialize all fields to unspecified
        (do ((i 0 (+ i 1)))
            ((= i num-fields))
          (vector-set! field-vec i (if #f #f)))
        ;; Set the constructor-provided fields
        (do ((args args (cdr args))
             (indices field-indices (cdr indices)))
            ((null? args))
          (vector-set! field-vec (car indices) (car args)))
        (%make-record rtd field-vec)))))

;; Helper macro to define accessor and optional mutator for a field
(define-syntax %define-field-accessors
  (syntax-rules ()
    ;; Field with accessor only
    ((%define-field-accessors rtd field-name accessor)
     (define accessor
       (let ((idx (%record-type-field-index rtd 'field-name)))
         (lambda (record)
           (%record-ref record idx)))))
    ;; Field with accessor and mutator
    ((%define-field-accessors rtd field-name accessor mutator)
     (begin
       (define accessor
         (let ((idx (%record-type-field-index rtd 'field-name)))
           (lambda (record)
             (%record-ref record idx))))
       (define mutator
         (let ((idx (%record-type-field-index rtd 'field-name)))
           (lambda (record value)
             (%record-set! record idx value))))))))

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
       (define (pred obj)
         (and (%record? obj)
              (eq? (%record-type-of obj) name)))

       ;; Create the constructor
       (define constructor
         (%make-constructor name
                            '(constructor-field ...)
                            '(field-name ...)))

       ;; Create accessors and mutators for each field
       (%define-field-accessors name field-name accessor . maybe-mutator)
       ...))))
