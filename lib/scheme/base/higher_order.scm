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

;; (string-map proc string1 string2 ...) / (string-for-each proc string1 ...)
;; (vector-map proc vec1 ...)           / (vector-for-each proc vec1 ...)
;;
;; Defined here rather than used from (patina internal strings) and
;; (patina internal vectors) for the same reason `map` and `for-each` are: a
;; Rust higher-order primitive calls back into Scheme from inside a Rust frame,
;; and a continuation captured there does not survive. That made
;; `(make-for-each-generator string-for-each "abc")` -- a coroutine generator
;; yielding from the callback -- drop its first element silently.
(define (%shortest-string strings)
  (let loop ((ss (cdr strings)) (n (string-length (car strings))))
    (if (null? ss) n (loop (cdr ss) (min n (string-length (car ss)))))))

(define (string-for-each proc . strings)
  (if (null? strings)
      (error "string-for-each: requires at least one string argument")
      (let ((n (%shortest-string strings)))
        (let loop ((i 0))
          (if (< i n)
              (begin
                (apply proc (map (lambda (s) (string-ref s i)) strings))
                (loop (+ i 1)))
              (if #f #f))))))

(define (string-map proc . strings)
  (if (null? strings)
      (error "string-map: requires at least one string argument")
      (let* ((n (%shortest-string strings))
             (out (make-string n)))
        (let loop ((i 0))
          (if (< i n)
              (begin
                (string-set! out i (apply proc (map (lambda (s) (string-ref s i)) strings)))
                (loop (+ i 1)))
              out)))))

(define (%shortest-vector vectors)
  (let loop ((vs (cdr vectors)) (n (vector-length (car vectors))))
    (if (null? vs) n (loop (cdr vs) (min n (vector-length (car vs)))))))

(define (vector-for-each proc . vectors)
  (if (null? vectors)
      (error "vector-for-each: requires at least one vector argument")
      (let ((n (%shortest-vector vectors)))
        (let loop ((i 0))
          (if (< i n)
              (begin
                (apply proc (map (lambda (v) (vector-ref v i)) vectors))
                (loop (+ i 1)))
              (if #f #f))))))

(define (vector-map proc . vectors)
  (if (null? vectors)
      (error "vector-map: requires at least one vector argument")
      (let* ((n (%shortest-vector vectors))
             (out (make-vector n)))
        (let loop ((i 0))
          (if (< i n)
              (begin
                (vector-set! out i (apply proc (map (lambda (v) (vector-ref v i)) vectors)))
                (loop (+ i 1)))
              out)))))
