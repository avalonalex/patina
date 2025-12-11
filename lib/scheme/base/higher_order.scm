;; Higher-order list operations implemented in Scheme
;;
;; These implementations are CPS-compatible because they use
;; normal Scheme procedure application, which works correctly
;; with both direct and CPS evaluation modes.
;;
;; Moving these from Rust primitives to Scheme ensures that
;; call/cc and other continuations work properly inside map,
;; for-each, and similar higher-order functions.

;; Helper: extract first element from each list
(define (%map-cars lists)
  (if (null? lists)
      '()
      (cons (car (car lists))
            (%map-cars (cdr lists)))))

;; Helper: extract rest of each list
(define (%map-cdrs lists)
  (if (null? lists)
      '()
      (cons (cdr (car lists))
            (%map-cdrs (cdr lists)))))

;; Helper: check if any list is null
(define (%any-null? lists)
  (if (null? lists)
      #f
      (if (null? (car lists))
          #t
          (%any-null? (cdr lists)))))

;; (map proc list1 list2 ...)
;; Apply proc element-wise to the elements of the lists and
;; return a list of the results.
(define (map proc . lists)
  (if (null? lists)
      (error "map: requires at least one list argument")
      (let loop ((lists lists))
        (if (%any-null? lists)
            '()
            (cons (apply proc (%map-cars lists))
                  (loop (%map-cdrs lists)))))))

;; (for-each proc list1 list2 ...)
;; Apply proc element-wise for side effects only.
;; Returns an unspecified value.
(define (for-each proc . lists)
  (if (null? lists)
      (error "for-each: requires at least one list argument")
      (let loop ((lists lists))
        (if (%any-null? lists)
            (if #f #f)  ; unspecified value
            (begin
              (apply proc (%map-cars lists))
              (loop (%map-cdrs lists)))))))
