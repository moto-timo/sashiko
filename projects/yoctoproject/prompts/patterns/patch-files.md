# Patch Files in Recipes

## Required headers

Every `.patch` or `.diff` added to a layer must carry, in its header:

- `Upstream-Status:` with one of:
  - `Pending` (not yet sent upstream),
  - `Submitted [where]` (a mailing list URL or pull request),
  - `Backport [where]` (the upstream commit URL),
  - `Denied` (upstream rejected it; say why in the header),
  - `Inactive-Upstream [lastcommit: when (and/or) lastrelease: when]`,
  - `Inappropriate [reason]` (OE-specific, for example `[oe specific]` or
    `[configuration]`).
- `Signed-off-by:` from whoever added the patch to the layer.
- `CVE: CVE-YYYY-NNNN` for each CVE the patch fixes, so CVE tooling marks it
  as patched.

## SRC_URI wiring

- A patch has an effect only if a `SRC_URI` entry names it.
- When a patch is renamed or refreshed, check that the old file is gone and
  the new name matches `SRC_URI`.
- `file://` paths are searched in `FILESPATH`, which includes `${BPN}-${PV}`,
  `${BPN}` and `files` beside the recipe. A file in the wrong directory may
  still be found, or may be shadowed by another layer.
