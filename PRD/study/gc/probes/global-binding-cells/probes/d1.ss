(write (guard (e (#t 'caught)) (eval '(begin (error 'x "boom") (define reverse 5)) (interaction-environment)))) (newline)
(write (guard (e (#t 'caught)) (reverse '(1 2)))) (newline)
