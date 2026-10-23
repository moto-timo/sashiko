# Guide Index

Load guides from the prompt directory based on what the patch touches. A
patch can match several rows. Load **every** matching guide, not only the
most specific one.

The triggers column lists paths, variables and keywords. Matching any trigger
is enough.

## Cross-Cutting Patterns

| Pattern | Triggers | File |
|---------|----------|------|
| Shell in tasks and scripts | any `do_*()` shell task, `pkg_postinst`, `pkg_preinst`, `*.sh`, scripts under `files/` or a recipe directory, init scripts, wrapper scripts, `ROOTFS_POSTPROCESS_COMMAND`, `IMAGE_POSTPROCESS_COMMAND` | shell-tasks.md |
