;; Maintained smoke drivers for the pinned corpus, separate from upstream suites.
;; Counts are checked by the runner: an empty or truncated driver cannot pass.
(patina-compat-smokes
 (tests
  ((slug "chibi-binary-record") (assertions 5))
  ((slug "pfds-queue") (assertions 6))
  ((slug "pfds-heap") (assertions 6))
  ((slug "slib-format") (assertions 4))
  ((slug "slib-string-search") (assertions 9))
  ((slug "slib-string-case") (assertions 7))
  ((slug "slib-string-port") (assertions 6))
  ((slug "slib-line-io") (assertions 7))
  ((slug "slib-generic-write") (assertions 7))
  ((slug "slib-printf") (assertions 7))
  ((slug "srfi-63") (assertions 6))
  ((slug "macduffie-json") (assertions 6))))
