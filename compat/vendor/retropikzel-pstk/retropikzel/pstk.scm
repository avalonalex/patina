(define (temp-name)
  (let ((filename (create-temp-file "pstk.")))
    (delete-file filename)
    filename))
(define wish-display pipe-write-string)
(define wish-read (lambda (pipe)
                    (let ((result (pipe-read pipe)))
                      (if (eof-object? result)
                        result
                        (read (open-input-string result))))))

(define (shell-command program output-path input-path)
  (string-append program " < " output-path " 1> " input-path " & "))

(define wish-newline (lambda (pipe) (pipe-write-char #\newline pipe)))
(define wish-flush (lambda () #t)) ; No need to do anything
(define wish-read-line pipe-read-line)
(define input-pipe-path (temp-name))
(define output-pipe-path (temp-name))
(define input-pipe #f)
(define output-pipe #f)
(define (run-program program)
    (create-pipe input-pipe-path 0777)
    (create-pipe output-pipe-path 0777)
    (system (shell-command program output-pipe-path input-pipe-path))
    (set! input-pipe (open-input-pipe input-pipe-path))
    (set! output-pipe (open-output-pipe output-pipe-path))
    (list input-pipe output-pipe))

(define *wish-program* "tclsh")
(define *wish-debug-input* (if (get-environment-variable "PSTK_DEBUG") #t #f))
(define *wish-debug-output* (if (get-environment-variable "PSTK_DEBUG") #t #f))

(define *use-keywords?*
  (cond-expand
    (chicken #t)
    (kawa #t)
    (stklos #t)
    (else #f)))

(define (%keyword? x)
  (cond-expand
    (chicken (keyword? x))
    (kawa (keyword? x))
    (srfi-88 (keyword? x))
    (else (error "Keywords not supported" x))))

(define (%keyword->string x)
  (cond-expand
    (chicken (keyword->string x))
    (kawa (keyword->string x))
    (stklos (keyword->string x))
    (else (error "Keywords not supported" x))))

(define nl (string #\newline))

(define wish-input #f)

(define wish-output #f)

(define tk-is-running #f)

(define tk-ids+widgets '())

(define tk-widgets '())

(define commands-invoked-by-tk '())

(define inverse-commands-invoked-by-tk '())

(define in-callback #f)

(define callback-mutex #t)

(define ttk-widget-map '())

(define tk-init-string
  (let ((start-str '("package require Tk"
                     "if {[package version tile] != \"\"} {"
                     " package require tile"
                     "}"
                     ""
                     "namespace eval AutoName {"
                     " variable c 0"
                     " proc autoName {{result \\#\\#}} {"
                     " variable c"
                     " append result [incr c]"
                     " }"
                     " namespace export *"
                     "}"
                     ""
                     "namespace import AutoName::*"
                     ""
                     "proc callToScm {callKey args} {"
                     " global scmVar"
                     " set resultKey [autoName]"
                     " puts \"(call $callKey \\\"$resultKey\\\" $args)\""
                     " flush stdout"
                     " vwait scmVar($resultKey)"
                     " set result $scmVar($resultKey)"
                     " unset scmVar($resultKey)"
                     " set result"
                     "}"
                     ""
                     "proc tclListToScmList {l} {"
                     " switch [llength $l] {"
                     " 0 {"
                     " return ()"
                     " }"
                     " 1 {"
                     " if {[string range $l 0 0] eq \"\\#\"} {"
                     " return $l"
                     " }"
                     " if {[regexp {^[0-9]+$} $l]} {"
                     " return $l"
                     " }"
                     " if {[regexp {^[.[:alpha:]][^ ,\\\"\\'\\[\\]\\\\;]*$} $l]} {"
                     " return $l"
                     " }"
                     " set result \\\""
                     " append result\\"
                     " [string map [list \\\" \\\\\\\" \\\\ \\\\\\\\] $l]"
                     " append result \\\""
                     ""
                     " }"
                     " default {"
                     " set result {}"
                     " foreach el $l {"
                     " append result \" \" [tclListToScmList $el]"
                     " }"
                     " set result [string range $result 1 end]"
                     " return \"($result)\""
                     " }"
                     " }"
                     "}"
                     ""
                     "proc evalCmdFromScm {cmd {properly 0}} {"
                     " if {[catch {"
                     " set result [uplevel \\#0 $cmd]"
                     " } err]} {"
                     " puts \"(error \\\"[string map [list \\\\ \\\\\\\\ \\\" \\\\\\\"] $err]\\\")\""
                     " } elseif $properly {"
                     " puts \"(return [tclListToScmList $result])\""
                     " } else {"
                     " puts \"(return \\\"[string map [list \\\\ \\\\\\\\ \\\" \\\\\\\"] $result]\\\")\""
                     " }"
                     " flush stdout"
                     "}")))
    (do ((str start-str (cdr str)) ; turn into one string with \n between each line
         (res "" (string-append res (car str) nl)))
      ((null? str) res))))

(define report-error
  (lambda (x)
    (newline)
    (display x)
    (newline)))

(define option?
  (lambda (x)
    (or (and *use-keywords?* (%keyword? x))
        (and (symbol? x)
             (let* ((s (symbol->string x))
                    (n (string-length s)))
               (char=? #\: (string-ref s (- n 1))))))))

(define make-option-string
  (lambda (x)
    (if (and *use-keywords?* (%keyword? x))
      (string-append " -" (%keyword->string x))
      (let* ((s (symbol->string x))
             (option (string-append " -" (substring s 0 (- (string-length s) 1)))))
        option))))

(define improper-list->string
  (lambda (a first)
    (cond ((pair? a)
           (cons (string-append (if first "" " ")
                                (form->string (car a)))
                 (improper-list->string (cdr a) #f)))
          ((null? a) '())
          (else (list (string-append " . " (form->string a)))))))

(define form->string
  (lambda (x)
    (cond ((eq? #t x) "#t")
          ((eq? #f x) "#f")
          ((number? x) (number->string x))
          ((symbol? x) (symbol->string x))
          ((string? x) x)
          ((null? x) "()")
          ((pair? x)
           (string-append "("
                          (apply string-append
                                 (improper-list->string x #t))
                          ")"))
          ((eof-object? x) "#<eof>")
          ((and *use-keywords?* (%keyword? x)) (%keyword->string x))
          (else "#<unspecified>"))))

(define string-translate
  (lambda (s map)
    (letrec
      ((s-prepend (lambda (s1 s2)
                    (cond ((null? s1) s2)
                          (else (s-prepend (cdr s1) (cons (car s1) s2))))))
       (s-xlate (lambda (s r)
                  (cond ((null? s) (reverse r))
                        (else (let ((n (assv (car s) map)))
                                (cond (n (s-xlate (cdr s)
                                                  (s-prepend (string->list (cdr n)) r)))
                                      (else (s-xlate (cdr s)
                                                     (cons (car s) r))))))))))
      (list->string
        (s-xlate (string->list s) '())))))

(define string-trim-left
  (lambda (str)
    (cond ((string=? str "") "")
          ((string=? (substring str 0 1) " ")
           (string-trim-left (substring str 1
                                        (string-length str))))
          (else str))))

(define get-property
  (lambda (key args . thunk)
    (cond ((null? args)
           (cond ((null? thunk) #f)
                 (else ((car thunk)))))
          ((eq? key (car args))
           (cond ((pair? (cdr args)) (cadr args))
                 (else (report-error (list 'get-property key args)))))
          ((or (not (pair? (cdr args)))
               (not (pair? (cddr args))))
           (report-error (list 'get-property key args)))
          (else (apply get-property key (cddr args) thunk)))))

(define tcl-true?
  (let ((false-values
          `(0 "0" 'false "false" ,(string->symbol "0"))))
    (lambda (obj) (not (memv obj false-values)))))

(define widget?
  (lambda (x)
    (and (memq x tk-widgets) #t)))

(define call-by-key
  (lambda (key resultvar . args)
    (cond ((and in-callback (pair? callback-mutex)) #f)
          (else (set! in-callback (cons #t in-callback))
                (let* ((cmd (get-property key commands-invoked-by-tk))
                       (result (apply cmd args))
                       (str (string-trim-left
                              (scheme-arglist->tk-argstring
                                (list result)))))
                  (tk-set-var! resultvar str)
                  (set! in-callback (cdr in-callback))
                  result)))))

(define gen-symbol
  (let ((counter 0))
    (lambda ()
      (let ((sym (string-append "g" (number->string counter))))
        (set! counter (+ counter 1))
        (string->symbol sym)))))

(define widget-name
  (lambda (x)
    (let ((name (form->string x)))
      (cond ((member name ttk-widget-map)
             (string-append "ttk::" name))
            (else name)))))

(define make-widget-by-id
  (lambda (type id . options)
    (let
      ((result
         (lambda (command . args)
           (case command
             ((get-id) id)
             ((create-widget)
              (let* ((widget-type (widget-name (car args)))
                     (id-prefix (if (string=? id ".") "" id))
                     (id-suffix (form->string (gen-symbol)))
                     (new-id (string-append id-prefix "." id-suffix))
                     (options (cdr args)))
                (tk-eval
                  (string-append
                    widget-type
                    " "
                    new-id
                    (scheme-arglist->tk-argstring options)))
                (apply make-widget-by-id
                       (append (list widget-type new-id)
                               options))))
             ((configure)
              (cond ((null? args)
                     (tk-eval
                       (string-append id " " (form->string command))))
                    ((null? (cdr args))
                     (tk-eval
                       (string-append
                         id
                         " "
                         (form->string command)
                         (scheme-arglist->tk-argstring args))))
                    (else
                      (tk-eval
                        (string-append
                          id
                          " "
                          (form->string command)
                          (scheme-arglist->tk-argstring args)))
                      (do ((args args (cddr args)))
                        ((null? args) '())
                        (let ((key (car args)) (val (cadr args)))
                          (cond ((null? options)
                                 (set! options (list key val)))
                                ((not (memq key options))
                                 (set! options
                                   (cons key (cons val options))))
                                (else (set-car! (cdr (memq key options))
                                                val))))))))
             ((cget)
              (let ((key (car args)))
                (get-property
                  key
                  options
                  (lambda ()
                    (tk-eval
                      (string-append
                        id
                        " cget"
                        (scheme-arglist->tk-argstring args)))))))
             ((call exec)
              (tk-eval
                (string-trim-left
                  (scheme-arglist->tk-argstring args))))
             (else
               (tk-eval
                 (string-append
                   id
                   " "
                   (form->string command)
                   (scheme-arglist->tk-argstring args))))))))
      (set! tk-widgets (cons result tk-widgets))
      (set! tk-ids+widgets
        (cons (string->symbol id)
              (cons result tk-ids+widgets)))
      result)))

(define scheme-arg->tk-arg
  (lambda (x)
    (cond ((eq? x #f) " 0")
          ((eq? x #t) " 1")
          ((eq? x '()) " {}")
          ((option? x) (make-option-string x))
          ((widget? x) (string-append " " (x 'get-id)))
          ((and (pair? x) (procedure? (car x)))
           (let* ((lambda-term (car x))
                  (rest (cdr x))
                  (l (memq lambda-term
                           inverse-commands-invoked-by-tk))
                  (keystr (if l (form->string (cadr l))
                            (symbol->string (gen-symbol)))))
             (if (not l)
               (let ((key (string->symbol keystr)))
                 (set! inverse-commands-invoked-by-tk
                   (cons lambda-term
                         (cons key
                               inverse-commands-invoked-by-tk)))
                 (set! commands-invoked-by-tk
                   (cons key
                         (cons lambda-term
                               commands-invoked-by-tk)))))
             (string-append " {callToScm "
                            keystr
                            (scheme-arglist->tk-argstring rest)
                            "}")))
          ((procedure? x)
           (scheme-arglist->tk-argstring `((,x))))
          ((list? x)
           (cond ((eq? (car x) '+)
                  (let ((result (string-trim-left
                                  (scheme-arglist->tk-argstring
                                    (cdr x)))))
                    (cond ((string=? result "") " +")
                          ((string=? "{" (substring result 0 1))
                           (string-append
                             " {+ "
                             (substring result 1
                                        (string-length result))))
                          (else (string-append " +" result)))))
                 ((and (= (length x) 3)
                       (equal? (car x) (string->symbol "@"))
                       (number? (cadr x))
                       (number? (caddr x)))
                  (string-append
                    "@"
                    (number->string (cadr x))
                    ","
                    (number->string (caddr x))))
                 (else
                   (string-append
                     " {"
                     (string-trim-left
                       (scheme-arglist->tk-argstring x))
                     "}"))))
          ((pair? x)
           (string-append
             " "
             (form->string (car x))
             "."
             (form->string (cdr x))))
          ((string? x)
           (if (string->number x)
             (string-append " " x)
             (string-append
               " \""
               (string-translate x
                                 '((#\\ . "\\\\") (#\" . "\\\"")
                                                  (#\[ . "\\u005b") (#\] . "\\]")
                                                  (#\$ . "\\u0024")
                                                  (#\{ . "\\{") (#\} . "\\}")))
               "\"")))
          (else (string-append " " (form->string x))))))

(define scheme-arglist->tk-argstring
  (lambda (args)
    (apply string-append (map scheme-arg->tk-arg args))))

(define make-wish-func
  (lambda (tkname)
    (let ((name (form->string tkname)))
      (lambda args
        (tk-eval
          (string-append
            name
            (scheme-arglist->tk-argstring args)))))))

(define read-wish
  (lambda ()
    (let ((term (wish-read wish-output)))
      (cond ((and *wish-debug-output*
                  (not (eof-object? term)))
             (display "wish->scheme: ")
             (write term)
             (newline)))
      term)))

(define wish
  (lambda arguments
    (for-each
      (lambda (argument)
        (cond (*wish-debug-input*
                (display "scheme->wish: ")
                (display argument)
                (newline)))
        (wish-display argument wish-input)
        (wish-newline wish-input)
        (wish-flush))
      arguments)))

(define start-wish
  (lambda ()
    (let ((result (run-program *wish-program*)))
      (set! wish-input (cadr result))
      (set! wish-output (car result)))))


(define tk-eval
  (lambda (cmd)
    (wish (string-append
            "evalCmdFromScm \""
            (string-translate cmd
                              '((#\\ . "\\\\") (#\" . "\\\"")))
            "\""))
    (let again ((result (read-wish)))
      (cond ((eof-object? result) #t)
            ((not (pair? result))
             (report-error (string-append
                             "An error occurred inside Tcl/Tk" nl
                             " --> " (form->string result)
                             " " (form->string (wish-read-line wish-output))
                             )))
            ((eq? (car result) 'return)
             (cadr result))
            ((eq? (car result) 'call)
             (apply call-by-key (cdr result))
             (again (read-wish)))
            ((eq? (car result) 'error)
             (report-error (string-append
                             "An error occurred inside Tcl/Tk" nl
                             " " cmd nl
                             " --> " (cadr result))))
            (else (report-error result))))))

(define tk-id->widget
  (lambda (id)
    (get-property
      (string->symbol (form->string id))
      tk-ids+widgets
      (lambda ()
        (if (tcl-true? (tk/winfo 'exists id))
          (make-widget-by-id
            (tk/winfo 'class id)
            (form->string id))
          #f)))))

(define tk-var
  (lambda (varname)
    (tk-set-var! varname "")
    (string-append
      "::scmVar("
      (form->string varname)
      ")")))

(define tk-get-var
  (lambda (varname)
    (tk-eval
      (string-append
        "set ::scmVar("
        (form->string varname)
        ")"))))

(define tk-set-var!
  (lambda (varname value)
    (tk-eval
      (string-append
        "set ::scmVar("
        (form->string varname)
        ") {"
        (form->string value)
        "}"))))

; start: void -> tk
(define tk-start
  (lambda args ; optional argument for name of wish program
    (when (and (not (null? args))
               (= 1 (length args)))
      (set! *wish-program* (car args)))
    (start-wish)
    (wish tk-init-string)
    (set! tk-ids+widgets '())
    (set! tk-widgets '())
    (set! in-callback #f)
    (let ((tk (make-widget-by-id 'toplevel "." 'class: 'Wish)))
      (set! commands-invoked-by-tk '())
      (set! inverse-commands-invoked-by-tk '())
      (tk/wm 'protocol tk 'WM_DELETE_WINDOW tk-end)
      tk)))

(define tk-end
  (lambda ()
    (set! tk-is-running #f)
    (wish "after 200 exit")
    (close-pipe input-pipe)
    (close-pipe output-pipe)
    (delete-file input-pipe-path)
    (delete-file output-pipe-path)))

(define tk-dispatch-event
  (lambda ()
    (let ((tk-statement (read-wish)))
      (if (and (list? tk-statement)
               (eq? (car tk-statement) 'call))
        (apply call-by-key (cdr tk-statement))))))

(define loop
  (lambda (tk)
    (cond ((not tk-is-running)
           (if wish-output
             (tk/wm 'protocol tk 'WM_DELETE_WINDOW '())))
          (else (tk-dispatch-event)
                (loop tk)))))

(define tk-event-loop
  (lambda (tk)
    (set! tk-is-running #t)
    (loop tk)))

(define ttk-map-widgets
  (lambda (x)
    (cond ((eq? x 'all)
           (set! ttk-widget-map '("button" "checkbutton" "radiobutton"
                                  "menubutton" "label" "entry" "frame"
                                  "labelframe" "scrollbar" "notebook"
                                  "progressbar" "combobox" "separator"
                                  "scale" "sizegrip" "treeview")))
          ((eq? x 'none)
           (set! ttk-widget-map '()))
          ((pair? x) (set! ttk-widget-map
                       (map form->string x)))
          (else (report-error
                  (string-append
                    "Argument to TTK-MAP-WIDGETS must be "
                    "ALL, NONE or a list of widget types."))))))

(define string-split
  (lambda (c s)
    (letrec
      ((split (lambda (i k tmp res)
                (cond ((= i k)
                       (if (null? tmp) res (cons tmp res)))
                      ((char=? (string-ref s i) c)
                       (split (+ i 1) k "" (cons tmp res)))
                      (else (split (+ i 1) k
                                   (string-append tmp
                                                  (string (string-ref s i)))
                                   res))))))
      (reverse (split 0 (string-length s) "" '())))))

(define ttk-available-themes
  (lambda ()
    (string-split #\space (tk-eval "ttk::style theme names"))))

(define do-wait-for-window
  (lambda (w)
    (tk-dispatch-event)
    (cond ((equal? (tk/winfo 'exists w) "0") '())
          (else (do-wait-for-window w)))))

(define tk-wait-for-window
  (lambda (w)
    (let ((outer-allow callback-mutex))
      (set! callback-mutex #t)
      (do-wait-for-window w)
      (set! callback-mutex outer-allow))))

(define tk-wait-until-visible
  (lambda (w)
    (tk/wait 'visibility w)))

(define lock!
  (lambda ()
    (set! callback-mutex
      (cons callback-mutex #t))))

(define unlock!
  (lambda ()
    (if (pair? callback-mutex)
      (set! callback-mutex
        (cdr callback-mutex)))))

(define tk-with-lock
  (lambda (thunk)
    (lock!)
    (thunk)
    (unlock!)))

(define tk/after (make-wish-func 'after))
(define tk/bell (make-wish-func 'bell))
(define tk/update (make-wish-func 'update))
(define tk/clipboard (make-wish-func 'clipboard))
(define tk/bgerror (make-wish-func 'bgerror))
(define tk/bind (make-wish-func 'bind))
(define tk/bindtags (make-wish-func 'bindtags))
(define tk/destroy (make-wish-func 'destroy))
(define tk/event (make-wish-func 'event))
(define tk/focus (make-wish-func 'focus))
(define tk/grab (make-wish-func 'grab))
(define tk/grid (make-wish-func 'grid))
(define tk/image (make-wish-func 'image))
(define tk/lower (make-wish-func 'lower))
(define tk/option (make-wish-func 'option))
(define tk/pack (make-wish-func 'pack))
(define tk/place (make-wish-func 'place))
(define tk/raise (make-wish-func 'raise))
(define tk/selection (make-wish-func 'selection))
(define tk/winfo (make-wish-func 'winfo))
(define tk/wm (make-wish-func 'wm))
(define tk/choose-color (make-wish-func "tk_chooseColor"))
(define tk/choose-directory (make-wish-func "tk_chooseDirectory"))
(define tk/dialog (make-wish-func "tk_dialog"))
(define tk/get-open-file (make-wish-func "tk_getOpenFile"))
(define tk/get-save-file (make-wish-func "tk_getSaveFile"))
(define tk/message-box (make-wish-func "tk_messageBox"))
(define tk/focus-follows-mouse (make-wish-func "tk_focusFollowsMouse"))
(define tk/focus-next (make-wish-func "tk_focusNext"))
(define tk/focus-prev (make-wish-func "tk_focusPrev"))
(define tk/popup (make-wish-func "tk_popup"))
(define tk/wait (lambda args (make-wish-func 'tkwait)))
(define tk/appname (make-wish-func "tk appname"))
(define tk/caret (make-wish-func "tk caret"))
(define tk/scaling (make-wish-func "tk scaling"))
(define tk/useinputmethods (make-wish-func "tk useinputmethods"))
(define tk/windowingsystem (make-wish-func "tk windowingsystem"))
(define ttk/available-themes ttk-available-themes)
(define ttk/set-theme (make-wish-func "ttk::style theme use"))
(define ttk/style (make-wish-func "ttk::style"))
