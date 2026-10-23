# False Positive Prevention Guide

A finding is only worth sending if a maintainer would agree it is wrong.
Dismiss a concern only with concrete evidence from the layer: the line that
sets the variable, the class that adds the dependency, the caller that
already passes the right arguments. Do not give the patch the benefit of the
doubt. Equally, do not report something you have not checked.

## Patterns that produce false positives

### 1. Values set somewhere else

A recipe that seems to lack `DEPENDS`, `LICENSE`, `S`, `SRC_URI` checksums or
`inherit` may get them from an `.inc` it `require`s, a class it inherits, or
a `.bbappend`. Before reporting a missing value, use `git_grep` on the
variable and read every `require`, `include` and `inherit` the recipe uses.
Many classes add build dependencies themselves: `autotools`, `meson`,
`cmake`, `pkgconfig`, `python3native`, `setuptools3`, `cargo` and others.

### 2. Spacing

Before reporting a spacing problem in an assignment, look at the exact value
inside the quotes, including continuation lines, and at the operator's
spacing rule in `overrides.md`.

### 3. Override syntax that does match

`VAR:class-native`, `VAR:${PN}-ptest`, `VAR:arm`, `VAR:libc-glibc`
and machine or distro names are valid overrides when that name is in
`OVERRIDES`, `MACHINEOVERRIDES` or `DISTROOVERRIDES`. Check before claiming an
override cannot match. The old underscore syntax (`VAR_append`) is a parse
error in current releases, so it is always a real finding.

### 4. Pre-existing issues

Do not report problems the patch neither introduces nor makes worse. If you
find a real one while reading, mark it `"preexisting": true`.

### 5. Upgrades that look like rewrites

A version upgrade can legitimately drop patches (merged upstream),
dependencies (no longer needed) and configure options. Check the upstream
change if you can see it. Report a drop only when something still needs the
removed piece, or when the commit message claims something the diff does not
do.

### 6. Style the tools already check

`oe-stylize.py`, oelint-adv and patchtest check variable order, indentation
and shortlog length. Report these only when they change behaviour, or when
the patch clearly breaks an OE rule maintainers enforce (see `severity.md`,
Low).

### 7. Build failures you have not traced

Do not claim a build failure without naming the configuration, the variable
or command that fails, and why. "This might not build on X" with no mechanism
is not a finding.

## Before you report

- Name the file, the variable or function, and the configuration where it
  goes wrong.
- Quote the line that proves it.
- Check `git_grep` for other places the same value is set.
