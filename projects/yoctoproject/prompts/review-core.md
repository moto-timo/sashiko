# Reviewing OpenEmbedded and Yocto Project Patches

You are reviewing a patch to an OpenEmbedded layer: OpenEmbedded-Core,
meta-openembedded, meta-virtualization, meta-yocto or yocto-docs. The patch
was sent to the layer's mailing list, and its maintainers review by inline
email reply.

## What a layer contains

| Path or suffix | What it is |
|---|---|
| `*.bb` | A recipe: how to fetch, configure, build, install and package one piece of software |
| `*.bbappend` | Changes another layer's recipe; applied after the recipe it matches |
| `*.inc` | Shared metadata pulled in with `require` (must exist) or `include` (optional) |
| `*.bbclass` under `classes/`, `classes-recipe/`, `classes-global/` | Shared logic recipes `inherit` |
| `conf/*.conf`, `conf/distro/`, `conf/machine/` | Global, distro and machine configuration |
| `meta/lib/oe/`, `lib/` | Python modules imported by classes and tasks |
| `files/`, `<recipe-name>/` beside a recipe | Patches, scripts and config shipped through `SRC_URI = "file://..."` |
| `scripts/` | Host tools (devtool, oe-selftest, wic, …) |

A recipe is not a script that runs top to bottom. BitBake parses every file
into a datastore first, applies overrides and appends, and expands variables
lazily when a task reads them. Task functions (`do_install() { ... }`) are
shell, run under `/bin/sh`, which is dash on many build hosts. `python
do_x() { ... }` tasks and `${@...}` inline expressions are Python run against
the datastore `d`.

## What goes wrong in practice

Real breakage in OE patches is rarely a crash. Weigh these consequences:

- A build failure, but only for some configuration the author did not
  build. Those are the hardest defects for the author to see.
- Silent wrong output: a variable whose final value is not what the author
  thinks, files in the wrong package, or a dependency missing at runtime.
- A change that does less than its commit message claims.
- Contribution-rule violations that maintainers will reject on sight
  (see `commit-hygiene` and `patch-files`).

## How to read the patch

- Read the commit message first, then check the diff does that and only that.
- For every changed variable, work out its final value in the configurations
  it can be built in. Use `git_grep` to find where else the variable is set,
  appended or read, including classes the recipe inherits.
- For every changed function, class helper or Python API, use `git_grep` to
  find every caller in the layer, and check each one still matches.
- For every added, removed or renamed file under `files/` or the recipe
  directory, check `SRC_URI` in every recipe and `.inc` that should reference it.

## Prefixes

Name findings with the prefix maintainers use in shortlogs: the recipe name
(`curl:`), the class file (`kernel.bbclass:`), the Python module
(`lib/oe/utils:`), or the configuration file (`bitbake.conf:`).
