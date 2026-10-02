import subprocess, json, os, sys
forms = ["(when #t 1)", "(when #t '(1 2))", "(if #t '(1 2) 0)", "(let () '(1 2))", "(case 3 ((3) 'b))", "(case 3 ((1 2) 'a) ((3) 'b) (else 'c))", "(guard (e (#t 0)) (raise 'x))", "(guard (e (#t 0)) 1)"]
out = open('results/formexp.txt', 'a')
for probe in ["form-eval", "form-eval-fresh"]:
    for f in forms:
        res = []
        for n in (1000, 3000):
            r = json.loads(subprocess.run(["python3", "run.py", "patina", probe, str(n), f], capture_output=True, text=True, env=dict(os.environ, PATINA_STATS="1")).stdout)
            res.append((n, round(r['max_rss_mib'], 1), r['wall_s'], r['rc'], r['stdout'].split('symbols . ')[1].split(')')[0] if 'symbols . ' in r['stdout'] else '?'))
        slope = (res[1][1] - res[0][1]) * 1024 * 1024 / 2000
        print(f"{probe:16} {f:42} {res}  slope={slope/1024:.2f} KiB/eval", file=out, flush=True)
