(package
  (maintainers "Takashi Kato<ktakashi@ymail.com>")
  (authors "Takashi Kato")
  (version "17.09.26")
  (library
    (name
      (postgresql))
    (path "postgresql.sld")
    (depends
      (postgresql apis)
      (postgresql conditions)))
  (library
    (name
      (postgresql apis))
    (path "postgresql/apis.sld")
    (cond-expand
      (sagittarius
        (depends
          (postgresql messages)
          (sagittarius)
          (rnrs)))
      (else
        (depends
          (postgresql buffer))))
    (cond-expand
      ((library (srfi 19))
        (depends
          (srfi 19)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme write)
      (scheme char)
      (postgresql digest md5)
      (postgresql misc socket)
      (postgresql misc ssl)
      (postgresql misc bytevectors)))
  (library
    (name
      (postgresql buffer))
    (path "postgresql/buffer.sld")
    (depends
      (scheme base)
      (scheme write)
      (postgresql messages)))
  (library
    (name
      (postgresql conditions))
    (path "postgresql/conditions.sld")
    (cond-expand
      ((library (rnrs))
        (depends
          (rnrs)))
      (else
        (depends
          (scheme base))))
    (depends))
  (library
    (name
      (postgresql digest md5))
    (path "postgresql/digest/md5.sld")
    (cond-expand
      (sagittarius
        (depends
          (math)
          (postgresql misc bytevectors)
          (rnrs)))
      (else
        (cond-expand
          ((library (rnrs))
            (depends
              (rnrs)))
          ((library (srfi 60))
            (depends
              (srfi 60)))
          ((library (srfi 33))
            (depends
              (srfi 33)))
          (else
            (depends)))
        (depends
          (postgresql misc bytevectors))))
    (depends
      (scheme base)))
  (library
    (name
      (postgresql messages))
    (path "postgresql/messages.sld")
    (cond-expand
      ((library (srfi 19))
        (depends
          (srfi 19)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme write)
      (postgresql conditions)
      (postgresql misc socket)
      (postgresql misc bytevectors)
      (postgresql misc io)))
  (library
    (name
      (postgresql misc bytevectors))
    (path "postgresql/misc/bytevectors.sld")
    (cond-expand
      (sagittarius
        (depends
          (rnrs)
          (sagittarius)
          (sagittarius control)))
      ((library (chibi bytevector))
        (depends
          (scheme base)
          (chibi bytevector)))
      (else
        (cond-expand
          ((library (srfi 60))
            (depends
              (srfi 60)))
          ((library (srfi 33))
            (depends
              (srfi 33))))
        (depends
          (scheme base)
          (scheme char))))
    (cond-expand
      (sagittarius
        (depends
          (util bytevector)))
      (else
        (depends)))
    (depends))
  (library
    (name
      (postgresql misc io))
    (path "postgresql/misc/io.sld")
    (cond-expand
      (sagittarius
        (depends
          (binary io)
          (rnrs)))
      (else
        (cond-expand
          ((library (srfi 60))
            (depends
              (srfi 60)))
          ((library (srfi 33))
            (depends
              (srfi 33))))
        (depends)))
    (depends
      (scheme base)
      (scheme case-lambda)))
  (library
    (name
      (postgresql misc socket))
    (path "postgresql/misc/socket.sld")
    (cond-expand
      ((library (srfi 106))
        (depends
          (srfi 106)))
      (chibi
        (depends
          (scheme base)
          (chibi net)
          (scheme cxr)
          (chibi filesystem)
          (chibi process)))
      (gauche
        (depends
          (gauche base)))
      (else
        (depends)))
    (depends))
  (library
    (name
      (postgresql misc ssl))
    (path "postgresql/misc/ssl.sld")
    (cond-expand
      (sagittarius
        (depends
          (rfc tls)))
      (else
        (depends)))
    (depends
      (scheme base)))
  (manual "README.md")
  (description "R7RS portable PostgreSQL binding"))
