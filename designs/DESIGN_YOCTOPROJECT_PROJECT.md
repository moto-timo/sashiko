# Design: Yocto Project and BitBake as Sashiko Projects

## 1. Overview

`DESIGN_MULTI_PROJECT_REVIEW.md` made the reviewed codebase a typed dimension
(`ProjectId`) and added Sashiko as a second project. This document adds two
more:

- **Yocto Project**: OpenEmbedded-Core, meta-openembedded (`openembedded-devel`),
  meta-virtualization, meta-yocto and
  yocto-docs. The content is mostly BitBake metadata: recipes, classes, `.inc`
  and `.conf` files, and patch files carried inside recipes.
- **BitBake**: the build tool itself. It is a Python codebase with its own
  list, tree, test suite and review concerns (datastore, parser, fetchers,
  server/cooker IPC).

The mailing list defines the split. BitBake is exactly what arrives on
`bitbake-devel`. `yoctoproject` is everything on the other OE and Yocto lists.
One Sashiko instance serves one project and reads one `mailing_lists.toml`, so
a list belongs to exactly one project, with no routing inside an instance.
The two projects then differ only in what follows from the list: the tree
(§5) and the analysis stages and prompts (§3.2, §4).

Both fit Sashiko's original shape better than Sashiko itself does. Patches
arrive on mailing lists that lore.kernel.org already archives, maintainers
review them inline by email, and a patch targets one of a few known trees and
branches.

### 1.1 Goals

1. `ProjectId::Yoctoproject` and `ProjectId::Bitbake`, selected with
   `--project yoctoproject` and `--project bitbake`.
2. One workflow module, `src/workflows/yoctoproject_patch_review.rs`, that
   serves both projects with a different analysis stage table for each (§3).
3. First-party prompt trees: `projects/yoctoproject/prompts/` and
   `projects/bitbake/prompts/`.
4. Ingestion through the existing lore/NNTP path. The lists start tracked for
   the web UI only, with all outbound email muted.
5. `yoctoproject` holds its several layer trees as remotes of one repository.
   BitBake has its own repository.
6. No behaviour change for the `linux` or `sashiko` projects.

### 1.2 Non-goals (deferred)

- Running patchtest or oelint-adv inside Sashiko. The standalone `bodnuti`
  tool covers them. See §7 for how the two meet later.
- Sending email to the OE, Yocto or BitBake lists. That needs the list
  maintainers' agreement and a muted trial period first.
- A pre-existing bug pipeline (`linux_bug.rs`) for either project. It is
  switched off for both by `has_bug_pipeline()` (§7 step 1).
- Recipe maintainer routing from `meta/conf/distro/include/maintainers.inc`.

## 2. Touchpoints (verified on `main` @ d7c69ae7)

Every `match` on `ProjectId` is exhaustive, so the compiler lists each site
that needs a new arm.

| File | What it decides | Yocto Project | BitBake |
|---|---|---|---|
| `src/project.rs` | `as_str`, `display_name`, `prompt_dir`, `uses_maintainers`, `mailing_lists_path`, `ALL`, `FromStr` | `yoctoproject`, `Yocto Project`, `yoctoproject`, `false`, `projects/yoctoproject/mailing_lists.toml` | `bitbake`, `BitBake`, `bitbake`, `false`, `projects/bitbake/mailing_lists.toml` |
| `src/workflows/mod.rs` | stage labels, stage count, planned stages | the new module with the Yocto Project stage table | the same module with the BitBake stage table |
| `src/worker/prompts.rs` | system prompt, series context, workflow builder | new `yoctoproject_*` functions, parameterized by project | same |
| `src/prompt_bundle.rs` | severity guide | `yoctoproject/severity.md` | `bitbake/severity.md` |
| `src/reviewer.rs:2082` | review tool attribution string | `sashiko:yoctoproject_patch_review` | `sashiko:bitbake_patch_review` |
| `src/main.rs`, `src/settings.rs`, `src/server.rs`, `src/local_review.rs`, `src/db.rs` | defaults, UI labels | mostly tests; add new arms where tests iterate `ALL` | same |
| `build.rs` | embeds `projects/*/prompts/` | no change | no change |

`uses_maintainers` is `false` for both. Neither tree has a kernel-format
`MAINTAINERS` file.

## 3. Workflow

### 3.1 Fork base

The two existing workflows differ in two ways that matter here:

| | `sashiko_patch_review.rs` | `linux_patch_review.rs` |
|---|---|---|
| Size | 1.9k lines; reuses the Linux validators (`validate_post_verification_batch_output` and others) instead of copying them | 4.4k lines; owns the shared validators and provenance enrichment |
| Stage table | an `ANALYSIS_STAGES` table that is easy to replace | kernel stages (locking, hardware) mixed into helpers |
| Report | `report` plus `summary`, GitHub PR comment format (`github-summary-template.md`) | `report`, LKML inline reply (`inline-template.md`, `validate_inline_format`) |

**Decision:** copy the sashiko workflow's structure, which is small and
borrows validators instead of copying them. Replace its report stage with the
Linux inline email report, because OE and BitBake maintainers review by
inline email reply. Make `validate_inline_format` `pub` in
`linux_patch_review.rs` instead of copying it. The `summary` stage is kept,
because the web UI shows it.

### 3.2 One module, two stage tables

Yocto Project and BitBake share everything except the analysis stages and
their prompts: state type, series handling, verification, post-verification,
report and summary. The module exports two tables. The dispatch in
`workflows/mod.rs` picks the table by `ProjectId`. This puts one new workflow
file in the tree instead of two near-identical ones. The precedent of one
independent workflow per project still holds between this pair and the
Linux and Sashiko workflows.

Yocto Project analysis stages:

| Stage | Looks for |
|---|---|
| `goal` | What the series claims to do; whether the commit message matches the diff |
| `commit-hygiene` | `<recipe>: <summary>` shortlog, `Signed-off-by`, upgrade messages listing license and behaviour changes, `[branch]` tag on stable backports |
| `patch-files` | `.patch` files added to `SRC_URI`: `Upstream-Status` value and justification, `CVE:` tags, `Signed-off-by`, patches left out of `SRC_URI` |
| `recipe-metadata` | Override syntax (`:append`, `:remove`, `:${PN}`), `+=` against `:append` ordering, `PV`/`SRCREV`/checksum consistency on upgrades, `LIC_FILES_CHKSUM` changes not explained, `DEPENDS` vs `RDEPENDS`, `inherit` misuse |
| `reproducibility` | Host contamination, absolute build paths, timestamps, `${B}`/`${S}`/`${D}` misuse, missing `PACKAGECONFIG` for automatic dependencies |
| `security` | CVE fixes that only cover part of the issue, `CVE_STATUS` used to hide a real issue, fetches over insecure transports |
| `python` | `meta/lib/oe` and class Python: datastore API use, `d.getVar` expansion, task signature changes |

BitBake analysis stages:

| Stage | Looks for |
|---|---|
| `goal` | As above |
| `implementation` | Logic matches intent; Python correctness |
| `datastore-parser` | Variable expansion, override and append semantics, parse cache and signature invalidation |
| `concurrency` | Server and cooker IPC, multiprocessing, the event queue, locks |
| `fetchers` | URL handling, untrusted input, checksum enforcement, network access |
| `compat` | Behaviour changes visible to layers: removed or renamed variables, warnings becoming errors, new required syntax |
| `tests` | `bitbake-selftest` coverage for the change |

Consolidation is the same as in the sashiko workflow: dedup, then
post-verification with the anti-charity rule, with `dismissed_concern`
output when a concern is dropped.

Short-circuit (`yoctoproject` only): when a series touches no BitBake metadata
(for example, docs only), skip `recipe-metadata`, `reproducibility` and
`python`.

## 4. Prompts

```
projects/yoctoproject/prompts/
  review-core.md  severity.md  false-positive-guide.md  inline-template.md
  subsystem/  oe-core.md  meta-oe.md  meta-virtualization.md  meta-yocto.md  yocto-docs.md
  patterns/   upstream-status.md  overrides.md  recipe-upgrade.md  licensing.md  cve.md
projects/bitbake/prompts/
  review-core.md  severity.md  false-positive-guide.md  inline-template.md
  subsystem/  datastore.md  fetch.md  server.md  runqueue.md
```

Sources are the OE patch and commit guidelines, the Yocto contributor guide,
the BitBake user manual, and patchtest's own checks. Patchtest's checks are
restated as review guidance here and are not run (§1.2).

## 5. Trees, lists and baselines

`yoctoproject` uses one repository at `third_party/yoctoproject`, configured
through `git.custom_remotes`:

| Remote | URL | lore list |
|---|---|---|
| `oe-core` | `https://git.openembedded.org/openembedded-core` | `openembedded-core` |
| `meta-oe` | `https://git.openembedded.org/meta-openembedded` | `openembedded-devel` |
| `meta-virt` | `https://git.yoctoproject.org/meta-virtualization` | `yocto-meta-virtualization` |
| `meta-yocto` | `https://git.yoctoproject.org/meta-yocto` | `yocto-patches` |
| `yocto-docs` | `https://git.yoctoproject.org/yocto-docs` | `yocto-docs` |

These histories are unrelated, and git handles that fine in one repository.
This is the same model as the kernel's `custom_remotes`.

BitBake uses `third_party/bitbake` (`https://git.openembedded.org/bitbake`).
Its `mailing_lists.toml` tracks `bitbake-devel` and nothing else. The
`yoctoproject` file must not list `bitbake-devel`, so no patch is reviewed
twice.

Baseline: list → remote, then the subject tag picks the branch
(`[scarthgap]`, `[kirkstone]`, `[meta-python][PATCH]`). For BitBake, release
branches look like `2.8`. Default is `master`. `yocto-patches` carries several
repositories, so it falls back to trying each remote's `master`. The lore list names
above were confirmed against `https://lore.kernel.org/manifest.js.gz` on
2026-10-23. The manifest also carries other layer lists
(`yocto-meta-arm`, `yocto-meta-ti`, `yocto-meta-freescale`,
`yocto-meta-arago`), which can be added later in the same way.

Both `mailing_lists.toml` files start with `track = true` and
`mute_all = true`: the web UI only.

## 6. Upstreaming

The aim is to land this upstream in `sashiko-dev/sashiko`, but only with the
agreement of both communities. The order is:

1. **Before code beyond step 1 below:** post this design as an RFC to
   `sashiko@lists.linux.dev` (or as a GitHub issue, per `CONTRIBUTING.md`).
   The question for the Sashiko maintainers is whether two more `ProjectId`s
   and one shared workflow module are welcome, and what they want in exchange
   (a benchmark suite, someone to maintain it).
2. **In parallel:** raise it with OE and Yocto maintainers (for example, on the
   OE-Core and Yocto technical calls, or the `openembedded-architecture`
   list), framed as web-UI-only reviews with no list email. Ask which checks
   they want and which they would find noisy. Their answers shape §3.2.
3. Keep each step below a separate, reviewable PR so upstream can take the
   project ID plumbing without committing to the prompts.

## 7. Implementation steps (one commit each)

1. `project: add Yocto Project and BitBake project ids`. This adds the enum
   variants and tests. Review dispatch arms are temporarily routed to the
   sashiko workflow. `--project yoctoproject|bitbake` parses and then stops
   at the existing "no prompts for project" check until steps 4–5 add the
   prompt trees. A new `ProjectId::has_bug_pipeline()` is `false` for both,
   so `process_issue_for_project` refuses them and the daemon starts no bug
   worker for them. Without that guard, their pre-existing concerns would be
   verified and filed with Sashiko's bug prompts.
2. `workflows: expose the inline report validator`.
3. `workflows: add Yocto Project family patch review workflow` (both stage
   tables, dispatch).
4. `projects: add Yocto Project prompt tree`.
5. `projects: add BitBake prompt tree`.
6. `projects: track Yocto Project and BitBake mailing lists` plus `Settings.toml`
   examples.
7. `baseline: map Yocto Project lists and branch tags to remotes`.
8. `benchmarks: add Yocto Project and BitBake smoke suites`, built from merged
   commits that had review findings on the list.

Each step runs `make check-pr` and the local
`sashiko review --project sashiko --agent`.

## 8. Relation to bodnuti

bodnuti (a separate repo) runs patchtest and oelint-adv and reports
deterministic findings. Later, its JSON report could feed this workflow as
pre-verified facts: a `checks` input to `goal`, so the LLM stages do not
repeat what a linter already proved. That is a separate design. Nothing here
depends on it.

## 9. Decisions and open questions

Decided:

- BitBake is its own `ProjectId` (§1).
- Names are `yoctoproject` and `bitbake`.
- Fork base: the sashiko workflow's structure with the Linux inline report
  (§3.1).
- Target: upstream, after the RFC in §6.

Open:

1. Should Yocto Project and BitBake share one workflow module (proposed, §3.2),
   or should each get its own, as Sashiko and Linux do? The RFC should ask
   upstream which they prefer.
2. Does yocto-docs belong to `yoctoproject`, or does it need its own stage
   table (prose, Sphinx) later?
