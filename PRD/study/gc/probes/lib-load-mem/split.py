import sys
s = open(sys.argv[1]).read()
forms = []; i = 0; n = len(s); depth = 0; start = None
while i < n:
    c = s[i]
    if c == ';':
        while i < n and s[i] != '\n': i += 1
        continue
    if s.startswith('#|', i):
        d = 1; i += 2
        while i < n and d:
            if s.startswith('|#', i): d -= 1; i += 2
            elif s.startswith('#|', i): d += 1; i += 2
            else: i += 1
        continue
    if s.startswith('#\\', i):
        if depth == 0 and start is None: start = i
        i += 3
        while i < n and s[i] not in ' \t\n()': i += 1
        if depth == 0: forms.append(s[start:i]); start = None
        continue
    if c == '"':
        if depth == 0 and start is None: start = i
        i += 1
        while i < n and s[i] != '"':
            i += 2 if s[i] == '\\' else 1
        i += 1
        if depth == 0: forms.append(s[start:i]); start = None
        continue
    if c in '([':
        if depth == 0: start = i
        depth += 1
    elif c in ')]':
        depth -= 1
        if depth == 0: forms.append(s[start:i+1]); start = None
    elif depth == 0 and not c.isspace():
        if start is None: start = i
        j = i
        while j < n and not s[j].isspace() and s[j] not in '()': j += 1
        # quote prefixes attach to next datum
        if s[i:j] in ("'", "`", ",", ",@", "#"): i = j; continue
        forms.append(s[start:j]); start = None; i = j; continue
    i += 1
sys.stdout.write("\n\x1e\n".join(forms))
