;;; Patina-specific patches for SRFI 1 reference implementation
;;;
;;; The reference implementation uses call/cc in %cdrs, %cars+cdrs, and
;;; %cars+cdrs+ as an "abort" optimization. This interacts poorly with
;;; library loading in the VM backend. These replacement versions avoid
;;; call/cc by checking for empty lists upfront.

;; Check if any list in LISTS is empty
(define (%any-list-null? lists)
  (let loop ((ls lists))
    (cond ((null? ls) #f)
          ((null? (car ls)) #t)
          (else (loop (cdr ls))))))

;; Override %cdrs — return cdrs of all lists, or '() if any is empty
(define (%cdrs lists)
  (if (%any-list-null? lists)
      '()
      (let recur ((lists lists))
        (if (pair? lists)
            (cons (cdar lists) (recur (cdr lists)))
            '()))))

;; Override %cars+cdrs — return (values cars cdrs), or (values '() '()) if any list is empty
(define (%cars+cdrs lists)
  (if (%any-list-null? lists)
      (values '() '())
      (let recur ((lists lists))
        (if (pair? lists)
            (let ((a (caar lists))
                  (d (cdar lists)))
              (receive (cars cdrs) (recur (cdr lists))
                (values (cons a cars) (cons d cdrs))))
            (values '() '())))))

;; Override %cars+cdrs+ — like %cars+cdrs but append cars-final to cars
(define (%cars+cdrs+ lists cars-final)
  (if (%any-list-null? lists)
      (values '() '())
      (let recur ((lists lists))
        (if (pair? lists)
            (let ((a (caar lists))
                  (d (cdar lists)))
              (receive (cars cdrs) (recur (cdr lists))
                (values (cons a cars) (cons d cdrs))))
            (values (list cars-final) '())))))
