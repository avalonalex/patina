(package
  (version "0.0.0")
  (library
    (name
      (chibi snow commands))
    (path "chibi/snow/commands.sld")
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (depends
      (scheme base)
      (scheme char)
      (scheme eval)
      (scheme file)
      (scheme lazy)
      (scheme load)
      (scheme process-context)
      (scheme time)
      (scheme read)
      (scheme write)
      (srfi 1)
      (srfi 27)
      (srfi 95)
      (chibi snow interface)
      (chibi snow package)
      (chibi snow utils)
      (chibi ast)
      (chibi bytevector)
      (chibi config)
      (chibi crypto md5)
      (chibi crypto rsa)
      (chibi crypto sha2)
      (chibi doc)
      (chibi filesystem)
      (chibi io)
      (chibi match)
      (chibi modules)
      (chibi net http)
      (chibi process)
      (chibi pathname)
      (chibi regexp)
      (chibi show)
      (chibi show pretty)
      (chibi string)
      (chibi sxml)
      (chibi system)
      (chibi tar)
      (chibi temp-file)
      (chibi uri)
      (chibi zlib)))
  (library
    (name
      (chibi snow fort))
    (path "chibi/snow/fort.sld")
    (cond-expand
      ((library (srfi 151))
        (depends
          (srfi 151)))
      ((library (srfi 33))
        (depends
          (srfi 33)))
      (else
        (depends
          (srfi 60))))
    (cond-expand
      (chibi
        (depends
          (chibi ast)
          (chibi)))
      (else
        (depends)))
    (depends
      (scheme base)
      (scheme read)
      (scheme write)
      (scheme file)
      (srfi 1)
      (srfi 18)
      (chibi snow package)
      (chibi bytevector)
      (chibi config)
      (chibi crypto rsa)
      (chibi filesystem)
      (chibi io)
      (chibi log)
      (chibi net servlet)
      (chibi pathname)
      (chibi regexp)
      (chibi string)
      (chibi sxml)
      (chibi tar)))
  (library
    (name
      (chibi snow interface))
    (path "chibi/snow/interface.sld")
    (cond-expand
      (chibi
        (depends
          (chibi filesystem)))
      (chicken
        (depends posix))
      (sagittarius
        (depends
          (sagittarius)
          (chibi string))))
    (depends
      (scheme base)
      (scheme char)
      (scheme read)
      (scheme write)
      (scheme file)
      (scheme process-context)
      (srfi 1)
      (chibi config)
      (chibi pathname)
      (chibi show)
      (chibi term edit-line)))
  (library
    (name
      (chibi snow package))
    (path "chibi/snow/package.sld")
    (depends
      (scheme base)
      (scheme char)
      (scheme file)
      (scheme read)
      (scheme write)
      (srfi 1)
      (srfi 115)
      (chibi snow interface)
      (chibi snow utils)
      (chibi bytevector)
      (chibi config)
      (chibi crypto md5)
      (chibi crypto rsa)
      (chibi crypto sha2)
      (chibi pathname)
      (chibi process)
      (chibi string)
      (chibi tar)
      (chibi uri)
      (chibi zlib)))
  (library
    (name
      (chibi snow utils))
    (path "chibi/snow/utils.sld")
    (cond-expand
      (chibi
        (depends
          (chibi io)))
      (chicken
        (depends)))
    (depends
      (scheme base)
      (scheme char)
      (scheme file)
      (scheme lazy)
      (scheme read)
      (scheme write)
      (scheme process-context)
      (srfi 1)
      (chibi config)
      (chibi char-set)
      (chibi net http)
      (chibi pathname)
      (chibi process)
      (chibi string)
      (chibi uri))))
