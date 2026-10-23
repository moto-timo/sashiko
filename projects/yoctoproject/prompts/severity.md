# Severity Levels

Assign a severity to every finding. State the reasoning at the start of
`severity_explanation`: what breaks, for which configurations, and how
likely a user is to hit it.

OpenEmbedded metadata is built by thousands of users in configurations the
author never tried. Calibrate against that: a defect that only shows up in
another architecture, libc or `PACKAGECONFIG` is still real, and it is
usually the one that matters most, because the author's own build passed.

## Critical

- The patch breaks parsing for every user of the layer (a syntax error in a
  global `.conf` or a widely inherited class, or a Python exception at parse
  time).
- The patch removes or weakens a security property for every build: disables
  checksum verification, fetches over an insecure transport by default, or
  marks a real unfixed CVE as not affecting the recipe.

## High

- The build fails for a common configuration the change reaches.
- The change silently does nothing, or does something other than what it
  claims.
- Runtime breakage on the target.
- A patch file missing `Upstream-Status`, or a CVE fix without a `CVE:` tag.
  Maintainers reject these on sight.

## Medium

- A build failure or wrong output for an uncommon but supported
  configuration (a non-default `PACKAGECONFIG`, a less common machine).
- Dead metadata the change leaves behind.
- Reproducibility or host-contamination problems: build paths, timestamps,
  or host tools leaking into the output.
- A commit message that misrepresents the change, or an upgrade whose
  message omits a license change or a behaviour change.

## Low

- Style rules OE enforces but that change no behaviour: shortlog format,
  variable ordering, line wrapping, whitespace in comments.
- A dependency that is redundant but harmless.
- Missing or vague commit message detail on an otherwise clear change.
