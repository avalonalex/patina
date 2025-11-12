# Scheme Test Suite

This directory contains Scheme test files for compatibility testing with various Scheme implementations.

## Directory Structure

- `chibi/` - Tests from chibi-scheme project
- `reports/` - Generated compatibility reports

## Attribution

### chibi-scheme r7rs-tests.scm

The file `chibi/r7rs-tests.scm` is sourced from the chibi-scheme project:
- **Source:** https://github.com/ashinn/chibi-scheme
- **File:** `tests/r7rs-tests.scm`
- **Copyright:** Copyright (c) 2009-2021 Alex Shinn
- **License:** BSD 3-Clause License

```
Copyright (c) 2009-2021 Alex Shinn
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.
3. The name of the author may not be used to endorse or promote products
   derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Running Tests

Use the test runner script:

```bash
./scripts/run_chibi_tests.sh
```

This will:
1. Build Patina in release mode (if needed)
2. Run the r7rs test suite
3. Generate a compatibility report in `reports/compatibility.md`

## Purpose

These tests help track Patina's R7RS compliance by comparing against the
comprehensive test suite from chibi-scheme, a well-established R7RS-small
implementation.
