# Shell in Tasks and Scripts

## Which shell runs it

- Shell task functions (`do_compile() { ... }`, `do_install:append() { ... }`)
  and package scripts (`pkg_postinst:${PN}() { ... }`) run under `/bin/sh`.
  On Debian and Ubuntu build hosts that is dash; on the target it is often
  BusyBox ash. Both are POSIX shells, not bash.
- A script shipped to the target with a `#!/bin/sh` shebang runs under the
  target's `/bin/sh`. A `#!/bin/bash` script needs `RDEPENDS:${PN} += "bash"`.

## Bashisms that break under dash or ash

| Construct | Behaviour under dash |
|---|---|
| `[[ ... ]]`, `==` inside `[ ]` | syntax error or wrong comparison; use `[ ... ]` and `=` |
| `var+=value` | syntax error; use `var="$var value"` |
| `${var/pattern/repl}`, `${var:offset:len}` | bad substitution |
| `function name { }` | not POSIX; use `name() { }` |
| arrays, `local -a`, `$'...'` strings | not supported |
| `source file` | use `. file` |

## Expansion mistakes

- BitBake expands `${VAR}` in shell tasks before the shell runs, but only
  for names that are BitBake variables. A shell variable is left for the
  shell to expand.
- Quote expansions that can contain spaces or be empty, especially in `[ ]`
  tests.
- `${@...}` is Python evaluated by BitBake at expansion time, not shell.

## Commands that differ between hosts

- `sed -i`, `find -printf`, `readlink -f` and `stat` options vary. Prefer
  forms that work with the versions in OE's `HOSTTOOLS`.
- Writing to `${D}` paths not listed in `FILES` produces installed-vs-shipped
  QA errors.
