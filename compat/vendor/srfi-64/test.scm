(import (scheme base) (scheme char) (scheme inexact) (scheme read) (scheme write) (scheme process-context) (scheme file) (scheme cxr) (srfi 64))

(define (list-sort less? xs) (if (null? xs) (quote ()) (let insert ((x (car xs)) (xs (list-sort less? (cdr xs)))) (if (null? xs) (list x) (let ((y (car xs)) (ys (cdr xs))) (if (less? x y) (cons x xs) (cons y (insert x ys))))))))

(define (written x) (cond-expand (r7rs (call-with-port (open-output-string) (lambda (out) (write x out) (get-output-string out)))) (else (call-with-output-string (lambda (out) (write x out))))))

(define (symbol<? a b) (string<? (symbol->string a) (symbol->string b)))

(define (call-with-false-on-error proc) (guard (_ (else #f)) (proc)))

(test-begin "srfi-64")

(define (prop-runner props thunk) (let ((r (test-runner-null)) (plist (quote ()))) (test-runner-on-test-end! r (lambda (runner) (set! plist (test-result-alist runner)))) (test-with-runner r (thunk)) (map (lambda (k) (assq k plist)) props)))

(define (on-test-runner thunk visit) (let ((r (test-runner-null)) (results (quote ()))) (test-runner-on-test-end! r (lambda (runner) (set! results (cons (visit r) results)))) (test-with-runner r (thunk)) (reverse results)))

(define (triv-runner thunk) (let ((r (test-runner-null)) (accum-pass (quote ())) (accum-fail (quote ())) (accum-xfail (quote ())) (accum-xpass (quote ())) (accum-skip (quote ()))) (test-runner-on-bad-count! r (lambda (runner count expected-count) (error (string-append "bad count " (number->string count) " but expected " (number->string expected-count)) (quote ())))) (test-runner-on-bad-end-name! r (lambda (runner begin end) (error (string-append "bad end group name " end " but expected " begin) (quote ())))) (test-runner-on-test-end! r (lambda (runner) (let ((n (test-runner-test-name runner))) (case (test-result-kind runner) ((pass) (set! accum-pass (cons n accum-pass))) ((fail) (set! accum-fail (cons n accum-fail))) ((xpass) (set! accum-xpass (cons n accum-xpass))) ((xfail) (set! accum-xfail (cons n accum-xfail))) ((skip) (set! accum-skip (cons n accum-skip))))))) (test-with-runner r (thunk)) (list (reverse accum-pass) (reverse accum-fail) (reverse accum-xfail) (reverse accum-xpass) (reverse accum-skip) (list (test-runner-pass-count r) (test-runner-fail-count r) (test-runner-xfail-count r) (test-runner-xpass-count r) (test-runner-skip-count r)))))

(define (path-revealing-runner thunk) (let ((r (test-runner-null)) (seq (quote ()))) (test-runner-on-test-end! r (lambda (runner) (set! seq (cons (list (test-runner-group-path runner) (test-runner-test-name runner)) seq)))) (test-with-runner r (thunk)) (reverse seq)))

(test-begin "1. Simple test-cases")

(test-begin "1.1. test-assert")

(define (t) (triv-runner (lambda () (test-assert "a" #t) (test-assert "b" #f))))

(test-equal "1.1.1. Very simple" (quote (("a") ("b") () () () (1 1 0 0 0))) (t))

(test-equal "1.1.2. A test with no name" (quote (("a") ("") () () () (1 1 0 0 0))) (triv-runner (lambda () (test-assert "a" #t) (test-assert #f))))

(test-equal "1.1.3. Tests can have the same name" (quote (("a" "a") () () () () (2 0 0 0 0))) (triv-runner (lambda () (test-assert "a" #t) (test-assert "a" #t))))

(define (choke) (error "Intentional test error" (quote ())))

(test-equal "1.1.4. One way to FAIL is to throw an error" (quote (() ("a") () () () (0 1 0 0 0))) (triv-runner (lambda () (test-assert "a" (choke)))))

(test-end)

(test-begin "1.2. test-eqv")

(define (mean x y) (/ (+ x y) 2.0))

(test-equal "1.2.1.  Simple numerical equivalence" (quote (("c") ("a" "b") () () () (1 2 0 0 0))) (triv-runner (lambda () (test-eqv "a" (mean 3 5) 4) (test-eqv "b" (mean 3 5) 4.5) (test-eqv "c" (mean 3 5) 4.0))))

(test-end)

(test-end "1. Simple test-cases")

(test-begin "2. Tests for catching errors")

(test-begin "2.1. test-error")

(test-equal "2.1.1. Baseline test; PASS with no optional args" (quote (("") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-error (choke)))))

(test-equal "2.1.2. Baseline test; FAIL with no optional args" (quote (() ("") () () () (0 1 0 0 0))) (triv-runner (lambda () (test-error (vector-ref (quote #(1 2)) 0)))))

(test-equal "2.1.3. PASS with a test name and error type" (quote (("a") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-error "a" #t (choke)))))

(test-end "2.1. test-error")

(test-end "2. Tests for catching errors")

(test-begin "3. Test groups and paths")

(test-equal "3.1. test-begin with unspecific test-end" (quote (("b") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-begin "a") (test-assert "b" #t) (test-end))))

(test-equal "3.2. test-begin with name-matching test-end" (quote (("b") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-begin "a") (test-assert "b" #t) (test-end "a"))))

(test-end "3. Test groups and paths")

(test-begin "4. Handling set-up and cleanup")

(test-equal "4.1. Normal exit path" (quote (in 1 2 out)) (let ((ex (quote ()))) (triv-runner (lambda () (test-group-with-cleanup "foo" (set! ex (cons (quote in) ex)) (set! ex (cons 1 ex)) (set! ex (cons 2 ex)) (set! ex (cons (quote out) ex))))) (reverse ex)))

(test-equal "4.2. Exception exit path" (quote (in 1 out)) (let ((ex (quote ()))) (triv-runner (lambda () (test-error (triv-runner (lambda () (test-group-with-cleanup "foo" (set! ex (cons (quote in) ex)) (test-assert #t) (set! ex (cons 1 ex)) (test-assert #t) (choke) (test-assert #t) (set! ex (cons 2 ex)) (test-assert #t) (set! ex (cons (quote out) ex)))))))) (reverse ex)))

(test-end "4. Handling set-up and cleanup")

(test-begin "5. Test specifiers")

(test-begin "5.1. test-match-named")

(test-equal "5.1.1. match test names" (quote (("y") () () () ("x") (1 0 0 0 1))) (triv-runner (lambda () (test-skip (test-match-name "x")) (test-assert "x" #t) (test-assert "y" #t))))

(test-equal "5.1.2. but not group names" (quote (("z") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-skip (test-match-name "x")) (test-begin "x") (test-assert "z" #t) (test-end))))

(test-end)

(test-begin "5.2. test-match-nth")

(test-equal "5.2.1. skip the nth one after" (quote (("v" "w" "y" "z") () () () ("x") (4 0 0 0 1))) (triv-runner (lambda () (test-assert "v" #t) (test-skip (test-match-nth 2)) (test-assert "w" #t) (test-assert "x" #t) (test-assert "y" #t) (test-assert "z" #t))))

(test-equal "5.2.2. skip m, starting at n" (quote (("v" "w" "z") () () () ("x" "y") (3 0 0 0 2))) (triv-runner (lambda () (test-assert "v" #t) (test-skip (test-match-nth 2 2)) (test-assert "w" #t) (test-assert "x" #t) (test-assert "y" #t) (test-assert "z" #t))))

(test-end)

(test-begin "5.3. test-match-any")

(test-equal "5.3.1. basic disjunction" (quote (("v" "w" "z") () () () ("x" "y") (3 0 0 0 2))) (triv-runner (lambda () (test-assert "v" #t) (test-skip (test-match-any (test-match-nth 3) (test-match-name "x"))) (test-assert "w" #t) (test-assert "x" #t) (test-assert "y" #t) (test-assert "z" #t))))

(test-equal "5.3.2. disjunction is commutative" (quote (("v" "w" "z") () () () ("x" "y") (3 0 0 0 2))) (triv-runner (lambda () (test-assert "v" #t) (test-skip (test-match-any (test-match-name "x") (test-match-nth 3))) (test-assert "w" #t) (test-assert "x" #t) (test-assert "y" #t) (test-assert "z" #t))))

(test-end)

(test-begin "5.4. test-match-all")

(test-equal "5.4.1. basic conjunction" (quote (("v" "w" "y" "z") () () () ("x") (4 0 0 0 1))) (triv-runner (lambda () (test-assert "v" #t) (test-skip (test-match-all (test-match-nth 2 2) (test-match-name "x"))) (test-assert "w" #t) (test-assert "x" #t) (test-assert "y" #t) (test-assert "z" #t))))

(test-equal "5.4.2. conjunction is commutative" (quote (("v" "w" "y" "z") () () () ("x") (4 0 0 0 1))) (triv-runner (lambda () (test-assert "v" #t) (test-skip (test-match-all (test-match-name "x") (test-match-nth 2 2))) (test-assert "w" #t) (test-assert "x" #t) (test-assert "y" #t) (test-assert "z" #t))))

(test-end)

(test-end "5. Test specifiers")

(test-begin "6. Skipping selected tests")

(test-equal "6.1. Skip by specifier - match-name" (quote (("x") () () () ("y") (1 0 0 0 1))) (triv-runner (lambda () (test-begin "a") (test-skip (test-match-name "y")) (test-assert "x" #t) (test-assert "y" #f) (test-end))))

(test-equal "6.2. Shorthand specifiers" (quote (("x") () () () ("y") (1 0 0 0 1))) (triv-runner (lambda () (test-begin "a") (test-skip "y") (test-assert "x" #t) (test-assert "y" #f) (test-end))))

(test-begin "6.3. Specifier Stack")

(test-equal "6.3.1. Clearing the Specifier Stack" (quote (("x" "x") ("y") () () ("y") (2 1 0 0 1))) (triv-runner (lambda () (test-begin "a then b") (test-begin "a") (test-skip "y") (test-assert "x" #t) (test-assert "y" #f) (test-end) (test-begin "b") (test-assert "x" #t) (test-assert "y" #f) (test-end) (test-end))))

(test-equal "6.3.2. Inheriting the Specifier Stack" (quote (("x" "x") () () () ("y" "y") (2 0 0 0 2))) (triv-runner (lambda () (test-begin "a then b") (test-skip "y") (test-begin "a") (test-assert "x" #t) (test-assert "y" #f) (test-end) (test-begin "b") (test-assert "x" #t) (test-assert "y" #f) (test-end) (test-end))))

(test-end)

(test-begin "6.4. Short-circuit evaluation")

(test-equal "6.4.1. In test-match-all" (quote (("x") ("y" "x" "z") () () ("y") (1 3 0 0 1))) (triv-runner (lambda () (test-begin "a") (test-skip (test-match-all "y" (test-match-nth 2))) (test-assert "x" #t) (test-assert "y" #f) (test-assert "y" #f) (test-assert "x" #f) (test-assert "z" #f) (test-end))))

(test-equal "6.4.2. In separate skip-list entries" (quote (("x") ("x" "z") () () ("y" "y") (1 2 0 0 2))) (triv-runner (lambda () (test-begin "a") (test-skip "y") (test-skip (test-match-nth 2)) (test-assert "x" #t) (test-assert "y" #f) (test-assert "y" #f) (test-assert "x" #f) (test-assert "z" #f) (test-end))))

(test-begin "6.4.3. Skipping test suites")

(test-equal "6.4.3.1. Introduced using 'test-begin'" (quote (("x") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-begin "a") (test-skip "b") (test-begin "b") (test-assert "x" #t) (test-end "b") (test-end "a"))))

(test-expect-fail 1)

(test-equal "6.4.3.2. Introduced using 'test-group'" (quote (() () () () () (0 0 0 0 1))) (triv-runner (lambda () (test-begin "a") (test-skip "b") (test-group "b" (test-assert "x" #t)) (test-end "a"))))

(test-equal "6.4.3.3. Non-skipped 'test-group'" (quote (("x") () () () () (1 0 0 0 0))) (triv-runner (lambda () (test-begin "a") (test-skip "c") (test-group "b" (test-assert "x" #t)) (test-end "a"))))

(test-end)

(test-end)

(test-end "6. Skipping selected tests")

(test-begin "7. Expected failures")

(test-equal "7.1. Simple example" (quote (() ("x") ("z") () () (0 1 1 0 0))) (triv-runner (lambda () (test-assert "x" #f) (test-expect-fail "z") (test-assert "z" #f))))

(test-equal "7.2. Expected exception" (quote (() ("x") ("z") () () (0 1 1 0 0))) (triv-runner (lambda () (test-assert "x" #f) (test-expect-fail "z") (test-assert "z" (choke)))))

(test-equal "7.3. Unexpectedly PASS" (quote (() () ("y") ("x") () (0 0 1 1 0))) (triv-runner (lambda () (test-expect-fail "x") (test-expect-fail "y") (test-assert "x" #t) (test-assert "y" #f))))

(test-end "7. Expected failures")

(test-begin "8. Test-runner")

(define (with-factory-saved thunk) (let* ((saved (test-runner-factory)) (result (thunk))) (test-runner-factory saved) result))

(test-begin "8.1. test-runner-current")

(test-assert "8.1.1. automatically restored" (let ((a 0) (b 1) (c 2)) (triv-runner (lambda () (set! a (test-runner-current)) (triv-runner (lambda () (set! b (test-runner-current)))) (set! c (test-runner-current)))) (and (eq? a c) (not (eq? a b)))))

(test-end)

(test-begin "8.2. test-runner-simple")

(test-assert "8.2.1. default on-test hook" (eq? (test-runner-on-test-end (test-runner-simple)) test-on-test-end-simple))

(test-assert "8.2.2. default on-final hook" (eq? (test-runner-on-final (test-runner-simple)) test-on-final-simple))

(test-end)

(test-begin "8.3. test-runner-factory")

(test-assert "8.3.1. default factory" (eq? (test-runner-factory) test-runner-simple))

(test-assert "8.3.2. settable factory" (with-factory-saved (lambda () (test-runner-factory test-runner-null) (test-with-runner (test-runner-create) (lambda () (test-begin "a") (test-assert #t) (test-assert #f) (test-assert (choke)) (test-end "a"))) (eq? (test-runner-factory) test-runner-null))))

(test-end)

(test-begin "8.4. test-runner-create")

(test-end)

(test-begin "8.5. test-runner-factory")

(test-end)

(test-begin "8.6. test-apply")

(test-equal "8.6.1. Simple (form 1) test-apply" (quote (("w" "p" "v") () () () ("x") (3 0 0 0 1))) (triv-runner (lambda () (test-begin "a") (test-assert "w" #t) (test-apply (test-match-name "p") (lambda () (test-begin "p") (test-assert "x" #t) (test-end) (test-begin "z") (test-assert "p" #t) (test-end))) (test-assert "v" #t))))

(test-equal "8.6.2. Simple (form 2) test-apply" (quote (("w" "p" "v") () () () ("x") (3 0 0 0 1))) (triv-runner (lambda () (test-begin "a") (test-assert "w" #t) (test-apply (test-runner-current) (test-match-name "p") (lambda () (test-begin "p") (test-assert "x" #t) (test-end) (test-begin "z") (test-assert "p" #t) (test-end))) (test-assert "v" #t))))

(test-expect-fail 1)

(test-equal "8.6.3. test-apply with skips" (quote (("w" "q" "v") () () () ("x" "p" "x") (3 0 0 0 3))) (triv-runner (lambda () (test-begin "a") (test-assert "w" #t) (test-skip (test-match-nth 2)) (test-skip (test-match-nth 4)) (test-apply (test-runner-current) (test-match-name "p") (test-match-name "q") (lambda () (test-assert "x" #t) (test-assert "p" #t) (test-assert "q" #t) (test-assert "x" #f) 0)) (test-assert "v" #t))))

(test-end)

(test-begin "8.7. test-with-runner")

(test-end)

(test-begin "8.8. test-runner components")

(define (auxtrack-runner thunk) (let ((r (test-runner-null))) (test-runner-aux-value! r (quote ())) (test-runner-on-test-end! r (lambda (r) (test-runner-aux-value! r (cons (test-runner-test-name r) (test-runner-aux-value r))))) (test-with-runner r (thunk)) (reverse (test-runner-aux-value r))))

(test-equal "8.8.1. test-runner-aux-value" (quote ("x" "" "y")) (auxtrack-runner (lambda () (test-assert "x" #t) (test-begin "a") (test-assert #t) (test-assert "y" #f) (test-end))))

(test-end)

(test-end "8. Test-runner")

(test-begin "9. Test Result Properties")

(test-begin "9.1. test-result-alist")

(define (symbol-alist? l) (if (null? l) #t (and (pair? l) (pair? (car l)) (symbol? (caar l)) (symbol-alist? (cdr l)))))

(test-assert (symbol-alist? (car (on-test-runner (lambda () (test-assert #t)) (lambda (r) (test-result-alist r))))))

(test-assert (symbol-alist? (car (on-test-runner (lambda () (test-assert #t)) (lambda (r) (test-result-alist r))))))

(test-equal (quote ((result-kind . pass))) (prop-runner (quote (result-kind)) (lambda () (test-assert #t))))

(test-equal (quote ((result-kind . fail) (expected-value . 2) (actual-value . 3))) (prop-runner (quote (result-kind expected-value actual-value)) (lambda () (test-equal 2 (+ 1 2)))))

(test-end "9.1. test-result-alist")

(test-begin "9.2. test-result-ref")

(test-equal (quote (pass)) (on-test-runner (lambda () (test-assert #t)) (lambda (r) (test-result-ref r (quote result-kind)))))

(test-equal (quote (pass)) (on-test-runner (lambda () (test-assert #t)) (lambda (r) (test-result-ref r (quote result-kind)))))

(test-equal (quote (fail pass)) (on-test-runner (lambda () (test-assert (= 1 2)) (test-assert (= 1 1))) (lambda (r) (test-result-ref r (quote result-kind)))))

(test-end "9.2. test-result-ref")

(test-begin "9.3. test-result-set!")

(test-equal (quote (100 100)) (on-test-runner (lambda () (test-assert (= 1 2)) (test-assert (= 1 1))) (lambda (r) (test-result-set! r (quote foo) 100) (test-result-ref r (quote foo)))))

(test-end "9.3. test-result-set!")

(test-end "9. Test Result Properties")

(test-end "srfi-64")

(exit 0)

(exit 0)
