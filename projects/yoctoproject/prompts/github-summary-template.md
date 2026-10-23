# Inline Review Report Template

Produce a plain-text inline review report based on the findings provided.

## Text Formatting Rules (Strictly Enforced)

- **Plain text only.** No markdown, no backticks (`), no markdown headings
  (`#`), no bold/italics (`**` or `*`), and no fenced code blocks (` ``` `).
  Never quote function names, variable names, types, or file paths in backticks.
- **No section headers.** Do NOT include `Summary:`, `Findings:`, or any other
  headers or preamble. Output ONLY the plain bulleted list of findings (or
  `No issues found.` if there are no findings).
- **Wrap lines at 78 characters.** Every finding description must be
  hard-wrapped at 78 characters or fewer so the report fits cleanly inside an
  80-column terminal window. Indent wrapped continuation lines of a bullet item
  by 2 spaces.
- **No line numbers.** Never mention line numbers when referencing code
  locations. Name the file path and the variable, task or function in plain text
  instead.
- **Factual and concise.** Keep problem descriptions short, conscious, direct,
  and self-contained. State where the issue is (file and variable, task or function), what
  is wrong, and why it matters. Do not add greetings, praise, filler, or
  sign-offs.
- **Include every finding.** You MUST list every finding passed to this stage.
  Do not omit any finding.
- **Order findings by severity** from highest (`[CRITICAL]`) to lowest
  (`[LOW]`).
- **Empty line between findings.** Separate individual findings with a single
  empty line so each finding stands out clearly.

## Exact Output Structure

When findings are present, output ONLY a bulleted list where every bullet starts
with `- [<SEVERITY>]` and individual findings are separated by an empty line:

- [<SEVERITY>] <short, conscious problem description naming file and variable or function>

- [<SEVERITY>] <short, conscious problem description naming file and variable or function>

Where `<SEVERITY>` is strictly one of `CRITICAL`, `HIGH`, `MEDIUM`, or `LOW` in
uppercase square brackets. If a finding is pre-existing (not introduced by this
patch), note `(pre-existing)` at the start of its description.

If there are no findings at all, output exactly:

No issues found.

## Example (findings present)

- [HIGH] In meta/recipes-example/foo/foo_2.0.bb, SYSTEMD_SERVICE:${PN}
  names foo.service, but do_install no longer installs that unit after the
  upgrade, so do_package fails on systemd distros.

- [MEDIUM] In meta/recipes-example/foo/foo_2.0.bb, the LIC_FILES_CHKSUM
  checksum for COPYING changes, but the commit message does not say whether
  the license itself changed.

- [LOW] The shortlog does not use the recipe name as its prefix.

## Example (no issues found)

No issues found.
