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

;; Every procedure below splits on (null? (cdr seqs)): the dominant
;; single-sequence call gets a direct loop — one procedure call per element —
;; while the n-ary case keeps the generic apply-over-rebuilt-argument-list
;; loop. This is the same shape chibi's init-7.scm uses for map. Without the
;; split, every element costs a fresh argument list and an apply.

;; (map proc list1 list2 ...)
;; Apply proc element-wise to the elements of the lists and
;; return a list of the results.
(define (map proc . lists)
  (cond
    ((null? lists)
     (error "map: requires at least one list argument"))
    ((null? (cdr lists))
     (let loop ((ls (car lists)))
       (if (null? ls)
           '()
           (cons (proc (car ls)) (loop (cdr ls))))))
    (else
     (let loop ((lists lists))
       (if (%any-null? lists)
           '()
           (cons (apply proc (%map-cars lists))
                 (loop (%map-cdrs lists))))))))

;; (for-each proc list1 list2 ...)
;; Apply proc element-wise for side effects only.
;; Returns an unspecified value.
(define (for-each proc . lists)
  (cond
    ((null? lists)
     (error "for-each: requires at least one list argument"))
    ((null? (cdr lists))
     (let loop ((ls (car lists)))
       (if (null? ls)
           (if #f #f)
           (begin
             (proc (car ls))
             (loop (cdr ls))))))
    (else
     (let loop ((lists lists))
       (if (%any-null? lists)
           (if #f #f)  ; unspecified value
           (begin
             (apply proc (%map-cars lists))
             (loop (%map-cdrs lists))))))))

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
        (if (null? (cdr strings))
            (let ((s (car strings)))
              (let loop ((i 0))
                (if (< i n)
                    (begin (proc (string-ref s i)) (loop (+ i 1)))
                    (if #f #f))))
            (let loop ((i 0))
              (if (< i n)
                  (begin
                    (apply proc (map (lambda (s) (string-ref s i)) strings))
                    (loop (+ i 1)))
                  (if #f #f)))))))

(define (string-map proc . strings)
  (if (null? strings)
      (error "string-map: requires at least one string argument")
      (let* ((n (%shortest-string strings))
             (out (make-string n)))
        (if (null? (cdr strings))
            (let ((s (car strings)))
              (let loop ((i 0))
                (if (< i n)
                    (begin (string-set! out i (proc (string-ref s i))) (loop (+ i 1)))
                    out)))
            (let loop ((i 0))
              (if (< i n)
                  (begin
                    (string-set! out i (apply proc (map (lambda (s) (string-ref s i)) strings)))
                    (loop (+ i 1)))
                  out))))))

(define (%shortest-vector vectors)
  (let loop ((vs (cdr vectors)) (n (vector-length (car vectors))))
    (if (null? vs) n (loop (cdr vs) (min n (vector-length (car vs)))))))

(define (vector-for-each proc . vectors)
  (if (null? vectors)
      (error "vector-for-each: requires at least one vector argument")
      (let ((n (%shortest-vector vectors)))
        (if (null? (cdr vectors))
            (let ((v (car vectors)))
              (let loop ((i 0))
                (if (< i n)
                    (begin (proc (vector-ref v i)) (loop (+ i 1)))
                    (if #f #f))))
            (let loop ((i 0))
              (if (< i n)
                  (begin
                    (apply proc (map (lambda (v) (vector-ref v i)) vectors))
                    (loop (+ i 1)))
                  (if #f #f)))))))

(define (vector-map proc . vectors)
  (if (null? vectors)
      (error "vector-map: requires at least one vector argument")
      (let* ((n (%shortest-vector vectors))
             (out (make-vector n)))
        (if (null? (cdr vectors))
            (let ((v (car vectors)))
              (let loop ((i 0))
                (if (< i n)
                    (begin (vector-set! out i (proc (vector-ref v i))) (loop (+ i 1)))
                    out)))
            (let loop ((i 0))
              (if (< i n)
                  (begin
                    (vector-set! out i (apply proc (map (lambda (v) (vector-ref v i)) vectors)))
                    (loop (+ i 1)))
                  out))))))

;; ── Procedures that call back into the program ──────────────────────────────
;;
;; `member` and `assoc` with a comparator, and `call-with-port`, are Scheme for
;; the reason `map` is: a continuation captured inside the procedure the
;; program passed must carry the rest of the work *this* procedure owes, and a
;; Rust primitive's frame cannot be part of a continuation. Re-entered after
;; the primitive had returned, such a continuation found nothing to return
;; into: the VM answered with a stray internal value (`#<cell>`) and the
;; tree-walker abandoned the form, where chibi and Gauche, whose versions are
;; Scheme, resume the search or return the procedure's value (#471).
;;
;; Without a comparator there is nothing to call back, and the primitives
;; (`%member`, `%assoc`) keep that path. So do calls with too many arguments,
;; which the primitive rejects with its own arity error.
;;
;; The rest follows the primitives exactly: the comparator is called as
;; `(compare obj elem)`, an improper tail ends the search with #f, and `assoc`
;; passes over an entry that is not a pair.

(define (member obj lst . compare)
  (cond ((null? compare) (%member obj lst))
        ((null? (cdr compare)) (%member-by (car compare) obj lst))
        (else (apply %member obj lst compare))))

(define (%member-by same? obj lst)
  (and (pair? lst)
       (if (same? obj (car lst))
           lst
           (%member-by same? obj (cdr lst)))))

(define (assoc obj alist . compare)
  (cond ((null? compare) (%assoc obj alist))
        ((null? (cdr compare)) (%assoc-by (car compare) obj alist))
        (else (apply %assoc obj alist compare))))

(define (%assoc-by same? obj alist)
  (and (pair? alist)
       (let ((entry (car alist)))
         (if (and (pair? entry) (same? obj (car entry)))
             entry
             (%assoc-by same? obj (cdr alist))))))

;; R7RS §6.13.1: the port is closed if `proc` returns, and not if it does not
;; — an escape or a raise leaves it open for whatever receives control, as the
;; primitive did (`escape_from_primitive.rs`). Every value `proc` returns is
;; returned.
(define (call-with-port port proc)
  (if (not (port? port))
      (error "call-with-port expects a port as first argument" port))
  (call-with-values
    (lambda () (proc port))
    (lambda results
      (close-port port)
      (apply values results))))
