# Variable Assignment and Override Syntax

## Operators

| Form | Effect | Spacing |
|---|---|---|
| `VAR = "x"` | assign; expanded when read | — |
| `VAR := "x"` | assign; expanded now | — |
| `VAR ?= "x"` | assign if unset at this point in parsing | — |
| `VAR ??= "x"` | weak default; applied at the end of parsing if still unset | — |
| `VAR += "x"` / `VAR =+ "x"` | append / prepend immediately | inserts one space |
| `VAR .= "x"` / `VAR =. "x"` | append / prepend immediately | inserts nothing |
| `VAR:append = "x"` / `VAR:prepend = "x"` | append / prepend at the end of parsing, after all assignments | inserts nothing |
| `VAR:remove = "x"` | removes every whitespace-separated word equal to `x`, after appends | — |

Consequences reviewers catch:

- `:append` is applied after `=` and `+=` assignments regardless of file
  order. A later `VAR = "..."` does not discard an earlier `VAR:append`;
  a later `VAR += "..."` lands before it.
- `?=` after another `?=` has no effect. `??=` is overridden by any other
  assignment.
- `:remove` cannot be undone by a later assignment; it removes words, not
  substrings.

## Overrides

- An override suffix (`VAR:arm`, `VAR:class-native`, `VAR:${PN}-dev`,
  `VAR:pn-foo`) applies only when that name is in `OVERRIDES`, which
  includes `MACHINEOVERRIDES`, `DISTROOVERRIDES`, the architecture,
  `TARGET_OS`, `libc-*`, `class-*` and `pn-${PN}`.
- Overrides chain left to right: `VAR:append:arm` appends only on arm.
- A helper variable is just another variable. It has an effect only if
  something reads it.
- The underscore override syntax (`VAR_append`, `RDEPENDS_${PN}`) was
  replaced by `:` and is rejected by current BitBake.

## Variables with rules

- `S` and `B` default to directories under `WORKDIR` and `UNPACKDIR`.
  Current OE-Core rejects recipes that set `S = "${WORKDIR}/git"` or
  otherwise point `S` at a path outside `UNPACKDIR`; check the current rules
  in `meta/classes-global/base.bbclass` and `meta/conf/bitbake.conf` before
  accepting a new `S` assignment.
- `PV`, `PR`, `PE` and `SRCREV` determine the package version; an upgrade
  must keep `SRC_URI` checksums and `LIC_FILES_CHKSUM` consistent with the
  new version.
