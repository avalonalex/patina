(package
  (maintainers "Peter Lane <peter@peterlane.info>")
  (authors "Marc Battanyi and Bruce Butterfield")
  (version "1.0.0")
  (library
    (name
      (rebottled cl-pdf))
    (path "rebottled/cl-pdf.sld")
    (depends
      (scheme base)
      (scheme cxr)
      (scheme file)
      (rebottled cl-pdf-utils)
      (slib format)))
  (library
    (name
      (rebottled cl-pdf-utils))
    (path "rebottled/cl-pdf-utils.sld")
    (depends
      (scheme base)
      (scheme case-lambda)
      (scheme inexact)
      (rebottled pregexp)
      (slib common-list-functions)
      (slib format)
      (robin statistics)))
  (manual "rebottled-pdf.html")
  (description "Low level functions for generating PDF files"))
