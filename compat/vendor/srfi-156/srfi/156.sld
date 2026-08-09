;;> \title{SRFI-156 reference implementation}
;;> 
;;> This is the reference implementation from
;;> \hyperlink[https://srfi.schemers.org/srfi-156/srfi-156.html]{SRFI-156}
;;> wrapped in a snowball.
;;>
;;> It provides two macros, \scheme{is} and \scheme{isnt}, for writing
;;> expressions with binary comparison functions in infix form.
;;>
;;> The code is by Panicz Maciej Godek.
;;>
;;> This snowball was made by Robert Fisher.
;;>
;;> \hyperlink[https://github.com/fisherro/snow-srfi-156]{github repo}
(define-library (srfi 156)
  (export is isnt)
  (import (scheme base))
  (begin
    (define-syntax infix/postfix
      (syntax-rules ()
	((infix/postfix x somewhat?)
	 (somewhat? x))

	((infix/postfix left related-to? right)
	 (related-to? left right))

	((infix/postfix left related-to? right . likewise)
	 (let ((right* right))
	   (and (infix/postfix left related-to? right*)
		(infix/postfix right* . likewise))))))

    (define-syntax extract-placeholders
      (syntax-rules (_)
	((extract-placeholders final () () body)
	 (final (infix/postfix . body)))

	((extract-placeholders final () args body)
	 (lambda args (final (infix/postfix . body))))

	((extract-placeholders final (_ op . rest) (args ...) (body ...))
	 (extract-placeholders final rest (args ... arg) (body ... arg op)))

	((extract-placeholders final (arg op . rest) args (body ...))
	 (extract-placeholders final rest args (body ... arg op)))

	((extract-placeholders final (_) (args ...) (body ...))
	 (extract-placeholders final () (args ... arg) (body ... arg)))

	((extract-placeholders final (arg) args (body ...))
	 (extract-placeholders final () args (body ... arg)))))

    (define-syntax identity-syntax
      (syntax-rules ()
	((identity-syntax form)
	 form)))

    (define-syntax is
      (syntax-rules ()
	((is . something)
	 (extract-placeholders identity-syntax something () ()))))

    (define-syntax isnt
      (syntax-rules ()
	((isnt . something)
	 (extract-placeholders not something () ()))))))

; Original copyright notice from the SRFI follows:
;
; Copyright (C) Panicz Maciej Godek (2017). All Rights Reserved.
; Permission is hereby granted, free of charge, to any person obtaining a copy
; of this software and associated documentation files (the "Software"), to deal
; in the Software without restriction, including without limitation the rights
; to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
; copies of the Software, and to permit persons to whom the Software is
; furnished to do so, subject to the following conditions:
; 
; The above copyright notice and this permission notice shall be included in
; all copies or substantial portions of the Software.
; 
; THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
; IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
; FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
; AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
; LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
; OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE;
; SOFTWARE.
