# Yocto Project Review Prompts

First-party review prompts for patches to OpenEmbedded and Yocto Project
layers, used by the `yoctoproject` project profile (`--project yoctoproject`).
That profile covers OpenEmbedded-Core, meta-openembedded,
meta-virtualization, meta-yocto and yocto-docs. BitBake itself is the separate
`bitbake` profile.

## Layout

| Path | Loaded by |
|---|---|
| `review-core.md` | Orientation: what an OE layer is and how its patches are reviewed |
| `subsystem/subsystem.md` | The index the pre-screen stage selects guides from |
| `patterns/*.md` | Metadata concerns that cut across recipes and classes |
| `false-positive-guide.md`, `severity.md` | The verification stages |
| `github-summary-template.md` | The report stage |

A guide is selected by name. The workflow looks a selected name up in both
`subsystem/` and `patterns/`, so names must be unique across the two
directories. Guides that a stage loads itself (`overrides.md`,
`patch-files.md`, `packaging.md`, `python-metadata.md`) are not offered to
the pre-screen stage.

## Writing a guide

State only what is true of current OpenEmbedded master: BitBake syntax and
semantics, OE-Core class behaviour, and the contribution rules maintainers
enforce. Do not tune a guide to a benchmark entry. A guide that names a
benchmark recipe, or a bug that exists only in one, inflates the score
without improving reviews.
