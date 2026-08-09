(define-c-library libc
                  `("sys/types.h"
                    "sys/socket.h"
                    "sys/un.h"
                    "netinet/in.h"
                    "netdb.h"
                    "errno.h"
                    "fcntl.h"
                    "poll.h"
                    "string.h")
                  libc-name
                  '((additional-versions ("0" "6"))))

(define-c-procedure c-socket libc 'socket 'int '(int int int))
(define-c-procedure c-setsockopt libc 'setsockopt 'int '(int int int pointer int))
(define-c-procedure c-getaddrinfo libc 'getaddrinfo 'int '(pointer pointer pointer pointer))
(define-c-procedure c-connect libc 'connect 'int '(int pointer int))
(define-c-procedure c-bind libc 'bind 'int '(int pointer int))
(define-c-procedure c-listen libc 'listen 'int '(int int))
(define-c-procedure c-accept libc 'accept 'int '(int pointer pointer))
(define-c-procedure c-perror libc 'perror 'void '(pointer))
(define-c-procedure c-fcntl libc 'fcntl 'int '(int int int))
(define-c-procedure c-htons libc 'htons 'u16 '(u16))
(define-c-procedure c-send libc 'send 'int '(int pointer int int))
(define-c-procedure c-read libc 'read 'int '(int pointer int))
(define-c-procedure c-poll libc 'poll 'int '(pointer int int))
(define-c-procedure c-strcpy libc 'strcpy 'int '(pointer pointer))
(define-c-procedure c-close libc 'close 'int '(int))
(define-c-procedure c-shutdown libc 'shutdown 'int '(int int))


(define-record-type <socket>
  (make-socket file-descriptor)
  socket?
  (file-descriptor socket-file-descriptor))

(define *af-inet* 2)
(define *af-inet6* 10)
(define *af-unspec* 0)

(define *sock-stream* 1)
(define *sock-dgram* 2)

(define *ai-canonname* 2)
(define *ai-numerichost* 4)
(define *ai-v4mapped* 8)
(define *ai-all* 16)
(define *ai-addrconfig* 32)

(define *ipproto-ip* 0)
(define *ipproto-tcp* 6)
(define *ipproto-udp* 17)

(define *msg-peek* 17)
(define *msg-oob* 1)
(define *msg-waitall* 256)

(define *shut-rd* 0)
(define *shut-wr* 1)
(define *shut-rdwr* 2)

(define F-SETFL 4)
(define O-NONBLOCK 2048)
(define EINPROGRESS 115)
(define SOL-SOCKET 1)
(define SO-ERROR 4)
(define POLLIN 1)
(define POLLOUT 4)
(define INADDR-ANY 0)
(define SO-REUSEADDR 2)
(define SO-REUSEPORT 15)
(define AI-PASSIVE 1)

(define *sockaddr-size* 16)
(define *ai-family-size* 2)

(define socket-merge-flags (lambda flags (apply + flags)))
(define (socket-purge-flags base-flag . flags) (apply - (cons base-flag flags)))

(define (make-client-socket node service . args)
  (let* ((ai-family (if (>= (length args) 1) (list-ref args 0) *af-inet*))
         (ai-socktype (if (>= (length args) 2) (list-ref args 1) *sock-stream*))
         (ai-flags (if (>= (length args) 3)
                     (list-ref args 2)
                     (socket-merge-flags *ai-v4mapped* *ai-addrconfig*)))
         (ai-protocol
           (if (>= (length args) 4)
             (list-ref args 3)
             *ipproto-ip*)))
    (let* ((ai-family-offset (c-type-size 'int))
           (ai-socktype-offset (* (c-type-size 'int) 2))
           (ai-protocol-offset (* (c-type-size 'int) 3))
           (addrinfo-hints
             (let ((pointer (make-c-bytevector 128 0)))
               (c-bytevector-set! pointer 'int ai-family-offset ai-family)
               (c-bytevector-set! pointer 'int ai-socktype-offset ai-socktype)
               pointer))
           (addrinfo (make-c-bytevector 128 0))
           (addrinfo-result
             (call-with-address-of
               addrinfo
               (lambda (addrinfo-address)
                 (call-with-address-of
                   addrinfo-hints
                   (lambda (addrinfo-hints-address)
                     (c-getaddrinfo (string->c-utf8 node)
                                    (string->c-utf8 service)
                                    addrinfo-hints
                                    addrinfo-address))))))
           (socket-file-descriptor
             (c-socket
               (c-bytevector-ref addrinfo 'int ai-family-offset)
               (c-bytevector-ref addrinfo 'int ai-socktype-offset)
               (c-bytevector-ref addrinfo 'int ai-protocol-offset))))
      (when (< addrinfo-result 0)
        (c-perror (string->c-utf8 "make-client-socket (addrinfo) error"))
        (raise-continuable "make-client-socket (addrinfo) error"))
      (when (< socket-file-descriptor 0)
        (c-perror (string->c-utf8 "make-client-socket (socket) error"))
        (raise-continuable "make-client-socket (socket) error"))
      (when (< (c-fcntl socket-file-descriptor F-SETFL O-NONBLOCK) 0)
        (c-perror (string->c-utf8 "make-client-socket (fcntl) error"))
        (raise-continuable "make-client-socket (fcntl) error"))
      (letrec* ((ai-addr-offset (* (c-type-size 'int) 6))
                (ai-addrlen-offset (* (c-type-size 'int) 4))
                (connect-result
                  (c-connect socket-file-descriptor
                             (c-bytevector-ref addrinfo 'pointer ai-addr-offset)
                             (c-bytevector-ref addrinfo 'int ai-addrlen-offset)))
                (pollfd (make-c-bytevector 128 0)))
        (c-bytevector-set! pollfd 'uint 0 socket-file-descriptor)
        (c-bytevector-set! pollfd 'int 0 0)
        ;; FIXME No magic numbers, like 8 or 1 here. Put into variable with good name
        ;; TODO Why 8 works but 1 does not?
        (when (= (c-poll pollfd 8 5000) 0)
          (error "make-client-socket (poll) error")))
      (make-socket socket-file-descriptor))))

(define message-type
  (lambda names
    (if (null? names)
      #f
      (map
        (lambda (name)
          (cond ((equal? name 'none) #f)
                ((equal? name 'peek) *msg-peek*)
                ((equal? name 'oob) *msg-oob*)
                ((equal? name 'wait-all) *msg-waitall*)))
        names))))

(define (socket-send socket bv . flags)
  (let* ((msg (bytevector->c-bytevector bv))
         (msg-len (bytevector-length bv))
         (sent-count (c-send (socket-file-descriptor socket) msg msg-len 0)))
    (when (= sent-count -1)
      (c-perror (string->c-utf8 "socket-send error"))
      (raise-continuable "socket-send error"))
    sent-count))

(define (socket-recv-loop socket bytes-pointer size)
  (let ((read-result (c-read (socket-file-descriptor socket)
                             bytes-pointer
                             size)))
    (cond ((< read-result 1) (socket-recv-loop socket bytes-pointer size))
          (else
            (c-bytevector->bytevector bytes-pointer size)))))

(define (socket-recv socket size . flags)
  ;; TODO FIXME If connection is closed return empty bytevector
  (when (not (integer? size))
    (error "socket-recv error: size must be integer" size))
  (let* ((msg-type (if (null? flags)
                     (message-type 'none)
                     (apply message-type flags)))
         (bytes-pointer (make-c-bytevector size 0)))
    (socket-recv-loop socket bytes-pointer size)))

(define (socket-close socket)
  (when (not (socket? socket))
    (error "socket-close: Not a socket" socket))
  (c-close (socket-file-descriptor socket)))

(define (make-network-server-socket-old service ai-family ai-socktype ai-protocol)
  (let* ((socket-file-descriptor (c-socket ai-family ai-socktype 0))
         (node "127.0.0.1")
         (sockaddr
           (let* ((pointer (make-c-bytevector 128 0))
                  (pointer-address (c-bytevector->integer pointer))
                  (node-pointer (integer->c-bytevector
                                  (+ pointer-address *ai-family-size*))))
             (c-bytevector-set! pointer 'u16 0 *af-inet*)
             (c-bytevector-set! pointer
                                'u16
                                (c-type-size 'u16)
                                (c-htons (string->number service)))
             (c-bytevector-set! pointer 'u16 (* (c-type-size 'u16) 2) INADDR-ANY)
             ;(c-strcpy node-pointer (string->c-utf8 node))
             pointer))
         (option (let ((pointer (make-c-bytevector (c-type-size 'int))))
                   (c-bytevector-set! pointer 'int 0 1)
                   pointer))
         (sockaddr-size (+ *ai-family-size* (bytevector-length (string->utf8 node)))))
    (when (< socket-file-descriptor 0)
      (c-perror (string->c-utf8 "make-server-socket (socket) error"))
      (raise-continuable "make-server-socket (socket) error"))
    (when (< (c-setsockopt socket-file-descriptor SOL-SOCKET SO-REUSEADDR option (c-type-size 'int)) 0)
      (c-perror (string->c-utf8 "make-server-socket (setsockopt SO-REUSEADDR) error"))
      (raise-continuable "make-server-socket (setsockopt SO-REUSEADDR) error"))
    (when (< (c-setsockopt socket-file-descriptor SOL-SOCKET SO-REUSEPORT option (c-type-size 'int)) 0)
      (c-perror (string->c-utf8 "make-server-socket (setsockopt SO-REUSEPORT) error"))
      (raise-continuable "make-server-socket (setsockopt SO-REUSEPORT) error"))
    (when (< (c-bind socket-file-descriptor sockaddr *sockaddr-size*) 0)
      (c-perror (string->c-utf8 "socket-accept (bind) error"))
      (raise-continuable "socket-accept (bind) error"))
    (when (< (c-listen socket-file-descriptor 0) 0)
      (c-perror (string->c-utf8 "make-server-socket (listen) error"))
      (raise-continuable "make-server-socket (listen) error"))
    (make-socket socket-file-descriptor)))

(define (make-server-socket service . args)
  (let* ((ai-family (if (>= (length args) 1) (list-ref args 0) *af-inet*))
         (ai-socktype (if (>= (length args) 2) (list-ref args 1) *sock-stream*))
         (ai-protocol (if (>= (length args) 3) (list-ref args 2) *ipproto-ip*)))
    (let* ((ai-flags AI-PASSIVE)
           (ai-flags-offset 0)
           (ai-family-offset (c-type-size 'int))
           (ai-socktype-offset (* (c-type-size 'int) 2))
           (ai-protocol-offset (* (c-type-size 'int) 3))
           (addrinfo-hints
             (let ((pointer (make-c-bytevector 128 0)))
               (c-bytevector-set! pointer 'int ai-flags-offset ai-flags)
               (c-bytevector-set! pointer 'int ai-family-offset ai-family)
               (c-bytevector-set! pointer 'int ai-socktype-offset ai-socktype)
               pointer))
           (addrinfo (make-c-bytevector 128 0))
           (addrinfo-result
             (call-with-address-of
               addrinfo
               (lambda (addrinfo-address)
                 (call-with-address-of
                   addrinfo-hints
                   (lambda (addrinfo-hints-address)
                     (c-getaddrinfo (string->c-utf8 "0.0.0.0")
                                    (string->c-utf8 service)
                                    addrinfo-hints
                                    addrinfo-address))))))
           (socket-file-descriptor
             (c-socket
               (c-bytevector-ref addrinfo 'int ai-family-offset)
               (c-bytevector-ref addrinfo 'int ai-socktype-offset)
               (c-bytevector-ref addrinfo 'int ai-protocol-offset)))
           (option (let ((pointer (make-c-bytevector (c-type-size 'int))))
                     (c-bytevector-set! pointer 'int 0 1)
                     pointer))
           (ai-addr-offset (* (c-type-size 'int) 6))
           (ai-addr (c-bytevector-ref addrinfo 'pointer ai-addr-offset))
           (ai-addrlen-offset (* (c-type-size 'int) 4))
           (ai-addr-len (c-bytevector-ref addrinfo 'int ai-addrlen-offset)))
      (when (< addrinfo-result 0)
        (c-perror (string->c-utf8 "make-server-socket (addrinfo) error"))
        (raise-continuable "make-server-socket (addrinfo) error"))
      (when (< socket-file-descriptor 0)
        (c-perror (string->c-utf8 "make-server-socket (socket) error"))
        (raise-continuable "make-server-socket (socket) error"))
      (when (< (c-setsockopt socket-file-descriptor SOL-SOCKET SO-REUSEADDR option (c-type-size 'int)) 0)
        (c-perror (string->c-utf8 "make-server-socket (setsockopt SO-REUSEADDR) error"))
        (raise-continuable "make-server-socket (setsockopt SO-REUSEADDR) error"))
      (when (< (c-setsockopt socket-file-descriptor SOL-SOCKET SO-REUSEPORT option (c-type-size 'int)) 0)
        (c-perror (string->c-utf8 "make-server-socket (setsockopt SO-REUSEPORT) error"))
        (raise-continuable "make-server-socket (setsockopt SO-REUSEPORT) error"))
      (when (< (c-bind socket-file-descriptor ai-addr ai-addr-len) 0)
        (c-perror (string->c-utf8 "make-server-socket (bind) error"))
        (raise-continuable "make-server-socket (bind) error"))
      (when (< (c-listen socket-file-descriptor 5) 0)
        (c-perror (string->c-utf8 "make-server-socket (listen) error"))
        (raise-continuable "make-server-socket (listen) error"))
      (make-socket socket-file-descriptor))))

(define (socket-accept socket)
  (let* ((addrlen (let ((pointer (make-c-bytevector (c-type-size 'int))))
                    (c-bytevector-set! pointer 'int 0 128)
                    pointer))
         (client-sockaddr (make-c-bytevector 128 0))
         (accepted-socket (c-accept (socket-file-descriptor socket)
                                    client-sockaddr
                                    addrlen)))
    (when (< accepted-socket 0)
      (c-perror (string->c-utf8 "socket-accept (accept) error"))
      (raise-continuable "socket-accept (accept) error"))
    (make-socket accepted-socket)))

(define (call-with-socket socket thunk)
  (let ((result (apply thunk (list socket))))
    (socket-close socket)
    result))

(define (socket-shutdown socket how)
  (c-shutdown (socket-file-descriptor socket) how))

(define-syntax address-family
  (syntax-rules ()
    ((_ name)
     (cond ((symbol=? 'name 'inet) *af-inet*)
           ((symbol=? 'name 'inet6) *af-inet6*)
           ((symbol=? 'name 'unspec) *af-unspec*)
           ((symbol=? 'name 'unix) *af-unix*)
           (else (error "address-family: Unrecognized name" 'name))))))

(define-syntax address-info
  (syntax-rules ()
    ((_ names ...)
     (apply socket-merge-flags
            (map (lambda (name)
                   (cond ((symbol=? name 'canonname) *ai-canonname*)
                         ((symbol=? name 'numerichost) *ai-numerichost*)
                         ((symbol=? name 'v4mapped) *ai-v4mapped*)
                         ((symbol=? name 'all) *ai-all*)
                         ((symbol=? name 'addrconfig) *ai-addrconfig*)
                         (else (error "address-info: Unrecognized name" 'name))))
                 '(names ...))))))

(define-syntax socket-domain
  (syntax-rules ()
    ((_ name)
     (cond ((symbol=? 'name 'stream) *sock-stream*)
           ((symbol=? 'name 'datagram) *af-unix*)
           (else (error "socket-domain: Unrecognized name" 'name))))))

(define-syntax ip-protocol
  (syntax-rules ()
    ((_ name)
     (cond ((symbol=? 'name 'ip) *ipproto-ip*)
           ((symbol=? 'name 'tcp) *ipproto-tcp*)
           ((symbol=? 'name 'udp) *ipproto-udp*)
           (else (error "ip-protocol: Unrecognized name" 'name))))))

(define-syntax shutdown-method
  (syntax-rules ()
    ((_ names ...)
     (cond ((and (member 'read '(names ...))
                 (member 'write '(names ...)))
            *shut-rdwr*)
           ((symbol=? (member 'read '(names ...)) 'read) *shut-rd*)
           ((symbol=? (member 'write '(names ...)) 'write) *shut-wr*)
           (else (error "shutdown-method: Names must be either read, write or both"))))))
