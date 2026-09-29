;; #364: numeric underscores are an intentional default-enabled extension.
;; Follow SRFI 169 and Gauche: one underscore between digits in a component.
;; Chibi does not implement this extension. Gauche's separate extension that
;; reads some malformed numbers as symbols is not part of Patina's policy.
;; DIVERGENCES.tsv records the measured differences.
;; Measured 2026-09-29: Chibi 0.12 passes 4/16; Gauche 0.9.15 passes 15/16.
(import (scheme base) (scheme read) (scheme write) (srfi 64))

(test-begin "numeric-separators")

;; Check values and exactness through both APIs. Guard so an oracle without
;; the extension still runs all rows instead of aborting on the first read.
(define (check-cases cases)
  (let loop ((cases cases))
    (or (null? cases)
        (and
         (guard (e (else #f))
           (let* ((text (caar cases)) (expected (cadr (car cases)))
                  (port (open-input-string text))
                  (read-value (read port)) (converted (string->number text)))
             (and (number? read-value) (number? converted)
                  (= expected read-value) (= expected converted)
                  (eq? (exact? expected) (exact? read-value))
                  (eq? (exact? expected) (exact? converted))
                  (eof-object? (read port)))))
         (loop (cdr cases))))))

(test-assert "decimal integer separators"
  (check-cases '(("1_000" 1000) ("+1_000" 1000) ("-1_000" -1000)
                 ("0_0_1" 1)
                 ("123_456_789_012_345_678_901" 123456789012345678901))))
(test-assert "radix digits and prefix order"
  (check-cases '(("#b10_10" 10) ("#o7_7" 63) ("#d1_000" 1000)
                 ("#xAb_Cd" 43981) ("#x1_e" 30) ("#xF_F" 255)
                 ("#x#eA_B" 171) ("#e#xA_B" 171) ("#X#EA_B" 171)
                 ("#x-FF_EE" -65518))))
(test-assert "rational separators"
  (check-cases '(("1_0/2_0" 1/2) ("#xA_B/1_0" 171/16)
                 ("#e#o1_0/2" 4) ("#i1_0/4" 2.5))))
(test-assert "fractional digits and exact decimals"
  (check-cases '(("1_000.5" 1000.5) (".1_25" 0.125) ("1.2_5" 1.25)
                 ("1_0." 10.0) ("#e1_000.5" 2001/2) ("#e1.2_5" 5/4))))
(test-assert "exponent digits and precision markers"
  (check-cases '(("1_0e1_0" 1e11) ("1e+1_0" 1e10) ("1e-1_0" 1e-10)
                 ("1_0s2" 1000.0) ("#e1_2e-1" 6/5)
                 ("#e1e2_0" 100000000000000000000))))
(test-assert "rectangular complex components"
  (check-cases '(("+1_0i" +10i) ("1_0+2_0i" 10+20i)
                 ("1.2_5+2_0i" 1.25+20i)
                 ("#i1_0.5+2_0.25i" 10.5+20.25i)
                 ("#xA_B+C_Di" 171+205i))))
(test-assert "polar complex components"
  (check-cases '(("1_0@0_0" 10@0) ("1_0.5@0.0_0" 10.5@0))))
(test-equal "string conversion radix argument"
  '(10 63 43981 171 10)
  (list (string->number "10_10" 2) (string->number "7_7" 8)
        (string->number "AB_CD" 16) (string->number "#eA_B" 16)
        (string->number "#b10_10" 16)))
(test-equal "radix argument enforces digit boundaries"
  '(#f #f #f #f #f #f)
  (list (string->number "1_2" 2) (string->number "7_8" 8)
        (string->number "1_f" 10) (string->number "_FF" 16)
        (string->number "F__F" 16) (string->number "#e_ff" 16)))

(define malformed
  '("1__000" "1000_" "+1__0" "#x_ff" "#x#e_ff" "#e#x_ff" "#xF__F"
    "#b1_2" "#o7_8" "1_.0" "1._0" "1_e2" "1e_2" "1e+_2" "1e2_"
    "1_/2" "1/_2" "1_0/2_" "1_0+_2i" "1_0+2_i" "1_0@_0" "1_0@0_"
    "#e_10" "#e1_.2" "#b_10" "1_000abc"))
(test-equal "malformed separators are not numeric strings"
  (map (lambda (text) #f) malformed) (map string->number malformed))
(test-equal "malformed numeric tokens raise read errors"
  (map (lambda (text) #t) malformed)
  (map (lambda (text)
         (guard (e (else (read-error? e))) (read (open-input-string text)) #f))
       malformed))
(test-equal "identifier underscores retain their meaning"
  '("_1000" "+name_1" "+_1000" "._1000" "1_000")
  (map (lambda (text) (symbol->string (read (open-input-string text))))
       '("_1000" "+name_1" "+_1000" "._1000" "|1_000|")))
(test-equal "number output uses ordinary digits"
  '("1000" "1000")
  (let* ((n (read (open-input-string "1_000"))) (port (open-output-string)))
    (write n port) (list (number->string n) (get-output-string port))))
(test-equal "hex escapes do not gain digit separators"
  '(#t #t #t)
  (map (lambda (text)
         (guard (e (else (read-error? e))) (read (open-input-string text)) #f))
       '("#\\x4_1" "\"\\x4_1;\"" "|\\x4_1;|")))
(test-equal "separators preserve following datums"
  '(1000 (a . 20) 171)
  (let* ((port (open-input-string "1_000 (a . 2_0) #xA_B"))
         (a (read port)) (b (read port)) (c (read port)))
    (list a b c)))
(test-equal "bytevector elements accept numeric separators"
  #u8(10 255)
  (read (open-input-string "#u8(1_0 #xF_F)")))

(test-end)
