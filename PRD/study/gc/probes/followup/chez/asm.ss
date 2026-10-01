(parameterize ([#%$assembly-output (current-output-port)] [optimize-level 3])
  (compile '(lambda (p x) (set-car! p x))))
