# Dependencies and Packaging

## Dependencies

- `DEPENDS` is build time: recipes whose sysroot output this recipe needs to
  configure and compile. Native tools need the `-native` variant; target
  libraries and their `.pc` files need the target recipe.
- `RDEPENDS:${PN}` is run time on the target. Shared-library dependencies are
  added automatically by `shlibs`; scripts, interpreters (`bash`, `python3`,
  `perl`) and data packages are not.
- A dependency on a pkg-config module needs the recipe that installs that
  `.pc` file.
- A dependency that is only needed for an optional feature belongs in a
  `PACKAGECONFIG[feature]` entry, so builds without that feature do not pull
  it in and builds with it get it.
- Check `DISTRO_FEATURES` gating: a dependency on `x11`, `systemd`, `pam` or
  `opengl` usually needs `bb.utils.contains`,
  `bb.utils.filter` or a `PACKAGECONFIG` default tied to the feature.

## Packaging

- `PACKAGES` lists output packages in order. `FILES:<pkg>` is matched in
  that order, and the first package whose pattern matches a file takes it.
- `PACKAGES_DYNAMIC` with `do_split_packages` creates packages at build time
  from file names. Empty packages are not created unless
  `ALLOW_EMPTY:<pkg>` is set.
- Moving files between packages needs `RREPLACES`, `RCONFLICTS` or
  `RPROVIDES` when users upgrade from the old split.
- Installed files not matched by any `FILES` entry produce the
  installed-vs-shipped QA error.
