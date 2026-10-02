#include <chibi/eval.h>
#include <stdio.h>

static sexp mk(void) {
  sexp ctx = sexp_make_eval_context(NULL, NULL, NULL, 0, 0);
  sexp_load_standard_env(ctx, NULL, SEXP_SEVEN);
  sexp_load_standard_ports(ctx, NULL, stdin, stdout, stderr, 1);
  return ctx;
}

static void ev(sexp ctx, const char *label, const char *src) {
  sexp r = sexp_eval_string(ctx, src, -1, NULL);
  fprintf(stderr, "%s: ", label);
  sexp_write(ctx, r, sexp_current_error_port(ctx));
  sexp_flush(ctx, sexp_current_error_port(ctx));
  fprintf(stderr, "\n");
}

int main(void) {
  sexp_scheme_init();
  sexp a = mk(), b = mk();
  ev(a, "A redirect", "(begin (define s (open-output-string)) (current-output-port s))");
  ev(b, "B display", "(display \"written by B\")");
  ev(a, "A captured", "(get-output-string s)");
  ev(a, "A redirect in", "(current-input-port (open-input-string \"(from A)\"))");
  ev(b, "B read", "(read)");
  sexp c = mk();
  ev(c, "C display", "(display \" and by C\")");
  ev(a, "A captured", "(get-output-string s)");
  /* error route */
  sexp d = mk(), e = mk();
  ev(d, "D param+error", "(begin (define s (open-output-string)) (parameterize ((current-output-port s)) (car 5)))");
  ev(e, "E display", "(display \" written by E\")");
  ev(d, "D captured", "(get-output-string s)");
  fflush(stdout);
  return 0;
}
