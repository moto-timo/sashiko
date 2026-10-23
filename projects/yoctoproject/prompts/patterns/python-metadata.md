# Python in Metadata and lib/oe

## The datastore

- `d.getVar('VAR')` returns the expanded value, or `None` if unset. Code that
  calls `.split()` or string methods on it must handle `None`.
- `d.getVar('VAR', False)` returns the unexpanded value.
- `d.setVar`, `d.appendVar` and `d.prependVar` change the datastore; changes
  made in a task do not persist to other tasks.
- Inline `${@...}` and anonymous `python () { ... }` run at parse time, for
  every recipe that inherits the class. An exception there is a parse
  failure for the whole layer.

## Changing a helper or API

- When a function in `meta/lib/oe/` or a class changes, use `git_grep`
  across `meta/` (classes, recipes, `lib/`, `scripts/`, selftests) to find
  its callers.

## Helpers

- `bb.utils.contains(var, item, true, false, d)` matches whole words only.
- `oe.utils.conditional` and `bb.utils.filter` are the usual helpers for
  feature-based values.
