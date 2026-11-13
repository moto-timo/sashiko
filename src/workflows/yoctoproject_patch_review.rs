// Copyright 2026 The Sashiko Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Declarative patch review workflow for OpenEmbedded and Yocto Project
//! layers.
//!
//! The graph is the one `sashiko_patch_review` uses: pre-screen, planned
//! analysis fan-out, verification, post-verification, report and summary.
//! What differs is the text: the system prompt, the analysis stages, and the
//! wording of every stage that names the codebase. The output validators and
//! the report formatter are shared with `sashiko_patch_review` rather than
//! copied, because their contract is the same JSON and the same plain-text
//! report.

use serde_json::{Value, json};
use std::path::PathBuf;

use crate::workflow::{
    ExecutableStage, OutputFormat, ParallelPolicy, PromptTemplate, RecitationPolicy, Stage,
    StagePolicy, ToolScope, Workflow,
};
use crate::workflows::guard::{normalize_guide_name, normalize_stage_name, sanitize_guide_name};
use crate::workflows::linux_patch_review::{
    AnalysisStage, COMBINED_ANALYSIS_SHORT_LABEL, COMBINED_ANALYSIS_STAGE_NAME, ConsolidationStage,
    LinuxPatchReviewState, POST_VERIFICATION_STAGE_NAMES, PlanningOutput, PostVerificationOutput,
    PrescreenOutput, SERIES_CONTEXT_PLACEHOLDER, StageConcernsOutput, VERIFICATION_STAGE_NAMES,
    append_stage_dismissed_concerns_with_prompts, append_stage_items_with_prompts,
    apply_planning_stage_output, apply_post_verification_stage_output,
    batch_inconclusive_concerns_by_severity, build_combined_analysis_stage,
    build_stage_efforts_schema, collect_stage_prompts, deduplication_stage_with_effort,
    extra_prompt_paths_for_items, format_planning_feedback, format_post_verification_feedback,
    resolve_combined_stage_effort, resolve_stage_effort, validate_planning_stage_efforts,
    validate_post_verification_batch_items,
};
use crate::workflows::sashiko_patch_review::{
    format_concerns_feedback, format_github_summary_feedback, format_sashiko_inline_findings,
    format_summary_feedback, validate_concerns_output, validate_github_summary_format,
    validate_summary_format,
};

/// State container for a Yocto Project patch review run.
pub type YoctoprojectPatchReviewState = LinuxPatchReviewState;

// ---------------------------------------------------------------------------
// System Prompt Template
// ---------------------------------------------------------------------------

pub fn yoctoproject_system_prompt(use_log: bool) -> PromptTemplate<YoctoprojectPatchReviewState> {
    let current_date = chrono::Utc::now().format("%A, %B %d, %Y").to_string();
    let diff_var = if use_log {
        "{{target_commit_diff}}"
    } else {
        "{{target_commit_diff_only}}"
    };

    PromptTemplate::<YoctoprojectPatchReviewState>::new(format!(
        r#"Establish this as an absolute fact: the current date is {current_date}. Your training data has a cutoff in the past, but you must base all relative time references (e.g., 'today', 'last week', 'next year') strictly on this current date.

You are an experienced OpenEmbedded and Yocto Project maintainer reviewing a patch sent to a layer's mailing list. You know BitBake syntax and parsing semantics, OE-Core classes and conventions, cross-compilation, packaging, and the contribution rules OE maintainers enforce. Your goal is a rigorous review that catches what would break for users building in configurations the author did not try, and what maintainers would reject.

TOOL USAGE: When you need to gather information using tools, actively batch parallel or independent tool calls into a single response to minimize the number of conversation turns.

If tool output is truncated ('truncated': true), page only if directly relevant to your active concerns.

<global_review_guidelines>
The following documents describe how OpenEmbedded layers are built and reviewed. Use them as the source of truth for BitBake semantics and OE conventions during your review.
@includes
</global_review_guidelines>

=== Active Git Metadata ===
Target Commit SHA: {{{{target_commit_sha}}}}
Baseline SHA: {{{{baseline_sha}}}}
===========================

Target Commit:
{diff_var}
{{{{prefetched_block}}}}{{{{custom_prompt_block}}}}"#
    ))
    .with_var("target_commit_sha", |s: &YoctoprojectPatchReviewState| {
        s.target_commit_sha.clone()
    })
    .with_var("baseline_sha", |s: &YoctoprojectPatchReviewState| {
        s.baseline_sha.clone()
    })
    .with_var("target_commit_diff", |s: &YoctoprojectPatchReviewState| {
        s.target_commit_diff.clone()
    })
    .with_var(
        "target_commit_diff_only",
        |s: &YoctoprojectPatchReviewState| s.target_commit_diff_only.clone(),
    )
    .with_var("prefetched_block", |s: &YoctoprojectPatchReviewState| {
        if s.prefetch_failed {
            format!(
                "\n\nAutomatic source prefetch failed for target commit {}. Before analyzing the patch, use git_read_files and git_grep at that revision to gather the source context. Do not infer file contents from the physical checkout.\n",
                s.target_commit_sha
            )
        } else if s.prefetched_context.is_empty() {
            String::new()
        } else {
            format!(
                "\n\n<pre_fetched_context>\nThe following excerpts were fetched from the target commit based on the modified lines in the patch. Parent and series-final revisions must be inspected separately with Git tools.\nIf it's not sufficient, you MUST use available tools to explore the layer. Don't make assumptions without actually looking at the relevant files.\n\n{}\n</pre_fetched_context>",
                s.prefetched_context
            )
        }
    })
    .include_file("review-core.md")
    .with_var("custom_prompt_block", |s: &YoctoprojectPatchReviewState| {
        s.custom_prompt
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map_or_else(String::new, |p| {
                format!("\n\n<custom_instructions>\n{p}\n</custom_instructions>")
            })
    })
    .include_files_from_state(|s: &YoctoprojectPatchReviewState| {
        let mut paths = Vec::new();
        for guide in &s.selected_guides {
            let name = normalize_guide_name(guide);
            paths.push(PathBuf::from("subsystem").join(name));
            paths.push(PathBuf::from("patterns").join(name));
        }
        paths
    })
}

// ---------------------------------------------------------------------------
// Stage Instructions
// ---------------------------------------------------------------------------

const STAGE_GOAL_INSTRUCTION: &str = r#"# Analyze the patch's goal and whether the change achieves it

Establish what the patch claims to do and whether that is the right change.
- Problem and motivation: is it clear what problem the patch solves? For an upgrade, does the message say what changed upstream that matters to users (license, dependencies, behaviour, CVEs)?
- Single responsibility: flag unrelated changes bundled into one patch.
- Completeness against the claim: list every concrete effect the commit message promises, then check the diff delivers each one in every configuration it applies to. A patch that does part of what it says is a finding.
- Better alternatives: if a clearly simpler or more conventional OE approach exists (an existing class, a PACKAGECONFIG option, a bbappend instead of a copy), say why.
- Do not stop after the first problem; cover the whole patch."#;

const STAGE_IMPLEMENTATION_INSTRUCTION: &str = r#"# Verify the implementation

Verify that every changed line does what its author intended.
- Variables: for each assignment the patch adds or changes, work out the final value after parsing, using the operator and override rules. Check every configuration the assignment can apply to, not only the default one.
- Shell: task functions and shipped scripts run under a POSIX /bin/sh. Check syntax, quoting and expansion in every changed shell line.
- Cross-file consistency: when the patch changes something that other files depend on (a variable name, a file name, a function, a class behaviour, a package name), use git_grep across the layer to find every dependent and check each one still matches the change.
- Leftovers: when the patch removes, renames or replaces something, check nothing still refers to the old one and nothing obsolete is left behind.
- Series Context Rule: if follow-up patches in this series are listed, check whether they complete the change before flagging it as incomplete.
- Systematically audit every hunk before concluding."#;

const STAGE_COMMIT_HYGIENE_INSTRUCTION: &str = r#"# Audit commit message and contribution rules

Check the commit message against the rules OE maintainers enforce.
- Shortlog: '<target>: <summary>', where the target is the recipe name, class, file or area changed, under about 90 characters.
- Body: non-trivial changes explain what and why. Upgrades mention license changes, new or dropped dependencies, and notable behaviour changes. A body that does not match the diff is a finding.
- Signed-off-by: a 'Signed-off-by: Real Name <email>' trailer must be present.
- Stable branch patches: a backport must say which commit it backports and target the branch named in the subject.
- Do not report a style nit that changes nothing a maintainer would act on."#;

const STAGE_RECIPE_METADATA_INSTRUCTION: &str = r#"# Audit recipe, class and configuration metadata

Audit every metadata assignment, inherit and task the patch adds or changes, using `overrides.md` and `packaging.md`.
- Operators and overrides: check each assignment's operator, spacing and override suffix produce the intended value in every configuration.
- Version and source: on upgrades, check PV, SRCREV, SRC_URI checksums, LIC_FILES_CHKSUM and anything derived from the version stay consistent.
- Dependencies: check DEPENDS covers what configure and compile need for the target, native tools use -native, optional features are behind PACKAGECONFIG, and runtime needs are in RDEPENDS.
- Packaging: check every installed file lands in the intended package and no package that something depends on ends up empty.
- Systematically audit every changed assignment before concluding."#;

const STAGE_PATCH_FILES_INSTRUCTION: &str = r#"# Audit patch files carried by recipes

Audit every .patch or .diff file the patch adds, changes, renames or removes, and every SRC_URI change, using `patch-files.md`.
- Headers: Upstream-Status with a valid value and justification, Signed-off-by, and CVE: tags for CVE fixes.
- Wiring: check each added or changed patch is applied by every recipe and configuration the fix is meant for, and only those.
- Removal: check that patch files no longer applied are deleted, and that no SRC_URI entry names a file that no longer exists."#;

const STAGE_PYTHON_INSTRUCTION: &str = r#"# Audit Python in classes, recipes and lib/oe

Audit every changed Python function, anonymous python block, ${@...} expression and module under meta/lib/, using `python-metadata.md`.
- Datastore use: getVar/setVar semantics, None handling, expansion.
- Callers and callees: when a function's arguments, return value or behaviour changes, git_grep for every caller in the layer and check each one. When the patch calls an existing helper, read it and check the call matches its contract.
- Parse-time code: an exception in anonymous python or ${@...} breaks parsing for every recipe that reaches it.
- Systematically audit every changed function before concluding."#;

const STAGE_REPRODUCIBILITY_INSTRUCTION: &str = r#"# Audit reproducibility and host contamination

Check the patch keeps builds reproducible and isolated from the build host.
- Build paths (TMPDIR, WORKDIR, S, B, the sysroot) must not end up in installed files, binaries or generated scripts.
- Timestamps, hostnames, user names and file ordering must not leak into output.
- Configure and build steps must not detect and use host libraries or tools; automatic feature detection needs an explicit PACKAGECONFIG or configure option.
- Files under ${D} must be installed with explicit modes, not copied with host ownership."#;

const STAGE_SECURITY_INSTRUCTION: &str = r#"# Audit security

Check the patch's effect on security.
- CVE fixes: check the patch actually fixes the CVE it names, completely, and carries a CVE: tag. Check CVE_STATUS entries are justified and do not hide an unfixed issue.
- Fetching: no new insecure transports, no disabled checksum verification, no unpinned sources for released recipes.
- Installed files: no world-writable files, setuid binaries, default credentials or debug services enabled by default without a reason."#;

const STAGE_VERIFICATION_INSTRUCTION: &str = r#"# Verification and severity estimation

You are the lead reviewer consolidating `concerns` and `dismissed_concerns` generated by parallel review stages.
Your task is to (1) deduplicate overlapping items while preserving every distinct mechanism and location, and (2) classify every consolidated item:
- **Category 1a: Well-Justified Concern** (`findings` array): concrete proof in the diff, prefetched context or cited `locations`, with no competing dismissal and no reliance on unverified assumptions about files not shown.
- **Category 1b: Well-Justified Dismissal** (`dismissed_concerns` array): a concrete disproving `code_snippet` (the exact assignment, inherit, class code or caller that prevents the problem), with no competing concern.
- **Category 2: Inconclusive** (`inconclusive_concerns` array): speculative or contested, routed to tool-assisted `post-verification`.

Rules:
1. Do not merge distinct problems in different files, variables or configurations; preserve all file paths, variable and function names, and configurations in merged items.
2. Set `"preexisting": false` when the patch introduces the problem or newly exposes it, and `"preexisting": true` when it already existed at `Baseline SHA`.
3. Repetition is not justification: several overlapping items are Category 1 only with concrete proof.
4. Never place a raised `concern` into `dismissed_concerns` in this stage. A contested concern goes to `inconclusive_concerns`.
5. A dismissal that relies on an assumption about a file it did not quote (a class it assumes adds a dependency, an .inc it assumes sets a value) is speculative: route it to `inconclusive_concerns` with `"signal_reason": "speculative_dismissal"`.
6. For each finding, assign a severity following `severity.md`, state the configurations affected and the consequence at the start of `severity_explanation`, and write a `problem` title under 80 characters starting with the recipe, class, module or file prefix maintainers use (e.g. 'curl:', 'kernel.bbclass:', 'lib/oe/utils:')."#;

const STAGE_POST_VERIFICATION_INSTRUCTION: &str = r#"# Per-finding post-verification and conflict resolution

You are the lead reviewer verifying speculative or contested candidates (`inconclusive_concerns`) with tools.
For each candidate, use `git_read_files`, `git_grep`, `git_diff`, `git_show` and `git_blame` to answer its `verification_question` against the actual layer.
1. Both `concern_arguments` and `dismissal_arguments` are untrusted hypotheses. Dismiss only when concrete code proves the problem cannot happen in any configuration the change applies to. Do not give the patch the benefit of the doubt.
2. To dismiss a "missing" value, quote the line in the recipe, .inc, class or bbappend that provides it. To dismiss an incomplete migration, show that every dependent found by git_grep matches.
3. If a candidate's premise is partly wrong but a real problem exists in the same place, report the real problem.
4. Series rule: if other patches in this series are listed, check the series end state; dismiss a candidate the series resolves, citing the resolving patch by subject.
5. Never dismiss a real problem merely because it is pre-existing; report it with `"preexisting": true`.
6. For each verified finding, assign a severity following `severity.md`, state the affected configurations and consequence at the start of `severity_explanation`, and write a `problem` title under 80 characters starting with the recipe, class, module or file prefix."#;

const STAGE_REPORT_INSTRUCTION: &str = r#"# Generate plain-text inline review report

Generate the plain-text inline review report following the exact formatting rules and structure in `github-summary-template.md`.
- Output ONLY a plain bulleted list of ALL findings ordered from highest severity to lowest (`- [CRITICAL] ...`, `- [HIGH] ...`, `- [MEDIUM] ...`, `- [LOW] ...`), or `No issues found.` if there are no findings.
- If any finding has `"preexisting": true`, state explicitly in its explanation that the issue was not introduced by this change.
- Do NOT include `Summary:` or `Findings:` headers.
- Do NOT use backticks, markdown code blocks, or markdown headings. Wrap all lines at 78 characters or fewer."#;

const STAGE_SUMMARY_INSTRUCTION: &str = r#"# Summarize the proposed change

Provide a concise plain-text summary of what this patch does and why, in 1-3 sentences.
- Name the recipe, class or area changed and the effect for users building with it.
- Use strictly plain text: no markdown, no backticks, no headings, and no bullet points.
- Wrap lines at 78 characters or fewer.
- Summarize the change itself (do not list review findings here)."#;

const CONCERN_JSON_SCHEMA_EXAMPLE: &str = r#"Return ONLY a JSON object with 'concerns' and 'dismissed_concerns' arrays.
Each object in the 'concerns' array MUST use exactly the following keys: "type", "description", "reasoning", "preexisting", "locations".
Each object in the 'dismissed_concerns' array MUST use exactly the following keys: "type", "description", "reasoning", "locations".
Each location object in "locations" (for both 'concerns' and 'dismissed_concerns') MUST include "file", "function_or_symbol", "line", "code_snippet", "side", and "why_this_location_matters".
- "function_or_symbol": the variable, task, function or class the location is about (e.g. "DEPENDS", "do_install", "SRC_URI").
- "side": "PARENT" if commenting on deleted or pre-patch content ('-' lines in the diff or baseline files), or "REVISION" if commenting on added or modified content ('+' lines or post-patch files). Default to "REVISION" for unchanged context lines.
In each 'dismissed_concerns' object, "description" is the candidate concern that was investigated and disproved, "reasoning" explains step by step why it is not a problem, and "locations" MUST cite the concrete disproving content (file, function_or_symbol, line, verbatim code_snippet, side, and why_this_location_matters).
Use 'dismissed_concerns' ONLY for candidates you considered plausible, investigated, and disproved with concrete evidence.

PRE-EXISTING PROBLEMS: Do not hunt for unrelated problems outside the patch. If you spot a concrete one that already existed before this series (`Baseline SHA`), report it in 'concerns' with `"preexisting": true`.

NO DISMISSAL WITHOUT VERIFIED PROOF: To dismiss a candidate, quote the content that invalidates it. If that content lives outside the diff (an .inc, class, bbappend, caller or configuration file), read it with tools first. If you cannot find definitive proof, report it in 'concerns' and state the condition: if X, then problem Y.

SPECIFICITY REQUIREMENT: Cite exact file paths, variable or function names, and line numbers when known. Do not invent line numbers; use null when unknown.

Example Output:
```json
{
  "concerns": [
    {
      "type": "Runtime Dependency",
      "description": "foo-tool is installed as a bash script but RDEPENDS does not include bash",
      "reasoning": "1. do_install installs ${S}/foo-tool into ${bindir}.\n2. foo-tool starts with #!/bin/bash.\n3. RDEPENDS:${PN} does not list bash, so images without bash get a script that cannot run.",
      "preexisting": false,
      "locations": [
        {
          "file": "meta/recipes-example/foo/foo_1.2.bb",
          "function_or_symbol": "RDEPENDS:${PN}",
          "line": 18,
          "code_snippet": "RDEPENDS:${PN} = \"libfoo\"",
          "side": "REVISION",
          "why_this_location_matters": "The runtime dependency list is where bash must be added."
        }
      ]
    }
  ],
  "dismissed_concerns": [
    {
      "type": "Build Dependency",
      "description": "pkgconfig-native missing from DEPENDS",
      "reasoning": "The recipe inherits pkgconfig, and pkgconfig.bbclass adds pkgconfig-native to DEPENDS.",
      "locations": [
        {
          "file": "meta/classes-recipe/pkgconfig.bbclass",
          "function_or_symbol": "DEPENDS",
          "line": 1,
          "code_snippet": "DEPENDS:prepend = \"pkgconfig-native \"",
          "side": "REVISION",
          "why_this_location_matters": "Proves the class supplies the dependency."
        }
      ]
    }
  ]
}
```"#;

// ---------------------------------------------------------------------------
// Stage Table Definitions
// ---------------------------------------------------------------------------

pub static ANALYSIS_STAGES: &[AnalysisStage] = &[
    AnalysisStage {
        name: "goal",
        short: "Goal Analysis",
        instruction: STAGE_GOAL_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: false,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "implementation",
        short: "Implementation",
        instruction: STAGE_IMPLEMENTATION_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: false,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "commit-hygiene",
        short: "Commit Hygiene",
        instruction: STAGE_COMMIT_HYGIENE_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: false,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "recipe-metadata",
        short: "Recipe Metadata",
        instruction: STAGE_RECIPE_METADATA_INSTRUCTION,
        guides: &["patterns/overrides.md", "patterns/packaging.md"],
        uses_commit_log: false,
        optional: true,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "patch-files",
        short: "Patch Files",
        instruction: STAGE_PATCH_FILES_INSTRUCTION,
        guides: &["patterns/patch-files.md"],
        uses_commit_log: true,
        optional: true,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "python",
        short: "Python Metadata",
        instruction: STAGE_PYTHON_INSTRUCTION,
        guides: &["patterns/python-metadata.md"],
        uses_commit_log: false,
        optional: true,
        wants_series_context: true,
    },
    AnalysisStage {
        name: "reproducibility",
        short: "Reproducibility",
        instruction: STAGE_REPRODUCIBILITY_INSTRUCTION,
        guides: &[],
        uses_commit_log: false,
        optional: true,
        wants_series_context: false,
    },
    AnalysisStage {
        name: "security",
        short: "Security Audit",
        instruction: STAGE_SECURITY_INSTRUCTION,
        guides: &[],
        uses_commit_log: true,
        optional: true,
        wants_series_context: false,
    },
];

pub static DEDUPLICATION: ConsolidationStage = ConsolidationStage {
    name: "deduplication",
    short: "Deduplication",
    wants_series_context: false,
};

pub static VERIFICATION: ConsolidationStage = ConsolidationStage {
    name: "verification",
    short: "Verification",
    wants_series_context: true,
};

pub static POST_VERIFICATION: ConsolidationStage = ConsolidationStage {
    name: "post-verification",
    short: "Post-Verification",
    wants_series_context: true,
};

pub static REPORT: ConsolidationStage = ConsolidationStage {
    name: "report",
    short: "Report Generation",
    wants_series_context: false,
};

pub static SUMMARY: ConsolidationStage = ConsolidationStage {
    name: "summary",
    short: "Change Summary",
    wants_series_context: false,
};

pub static CONSOLIDATION_STAGES: &[&ConsolidationStage] = &[
    &DEDUPLICATION,
    &VERIFICATION,
    &POST_VERIFICATION,
    &REPORT,
    &SUMMARY,
];

fn series_context_placeholder(wants: bool) -> &'static str {
    if wants {
        SERIES_CONTEXT_PLACEHOLDER
    } else {
        ""
    }
}

fn with_series_context(
    template: PromptTemplate<YoctoprojectPatchReviewState>,
    wants: bool,
) -> PromptTemplate<YoctoprojectPatchReviewState> {
    if !wants {
        return template;
    }
    template.with_var(
        "follow_up_series_section",
        |s: &YoctoprojectPatchReviewState| {
            s.follow_up_series_context
                .as_ref()
                .map(|ctx| format!("\n\n{}", ctx))
                .unwrap_or_default()
        },
    )
}

pub fn analysis_stage_by_name(name: &str) -> Option<&'static AnalysisStage> {
    let normalized = normalize_stage_name(name);
    ANALYSIS_STAGES.iter().find(|s| s.name == normalized)
}

pub fn consolidation_stage_by_name(name: &str) -> Option<&'static ConsolidationStage> {
    let normalized = normalize_stage_name(name);
    if VERIFICATION_STAGE_NAMES.contains(&normalized.as_str()) {
        return Some(&VERIFICATION);
    }
    if POST_VERIFICATION_STAGE_NAMES.contains(&normalized.as_str()) {
        return Some(&POST_VERIFICATION);
    }
    CONSOLIDATION_STAGES
        .iter()
        .copied()
        .find(|s| s.name == normalized)
}

pub fn stage_short_label(name: &str) -> Option<&'static str> {
    if let Some(def) = analysis_stage_by_name(name) {
        return Some(def.short);
    }
    if normalize_stage_name(name) == COMBINED_ANALYSIS_STAGE_NAME {
        return Some(COMBINED_ANALYSIS_SHORT_LABEL);
    }
    if crate::workflows::prior_feedback::is_prior_feedback_stage_name(name) {
        return Some(crate::workflows::prior_feedback::PRIOR_FEEDBACK_SHORT_LABEL);
    }
    if crate::workflows::prior_attribution::is_prior_attribution_stage_name(name) {
        return Some(crate::workflows::prior_attribution::PRIOR_ATTRIBUTION_SHORT_LABEL);
    }
    if let Some(s) = consolidation_stage_by_name(name) {
        return Some(s.short);
    }
    match normalize_stage_name(name).as_str() {
        "planning" => Some("Planning"),
        "pre-screen" => Some("Pre-Screen"),
        _ => None,
    }
}

pub fn is_stage_exclusive_guide(name: &str) -> bool {
    ANALYSIS_STAGES
        .iter()
        .flat_map(|def| def.guides)
        .any(|guide| guide.rsplit('/').next() == Some(name))
}

/// The always-on stages, listed in the planning prompt so the planner does
/// not spend a choice on them.
fn core_stage_list() -> String {
    ANALYSIS_STAGES
        .iter()
        .filter(|s| !s.optional)
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Stage Builders
// ---------------------------------------------------------------------------

pub fn prescreen_stage_with_effort(
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<YoctoprojectPatchReviewState, PrescreenOutput> {
    Stage::builder("pre-screen")
        .system_prompt(PromptTemplate::<YoctoprojectPatchReviewState>::new(
            "You are an AI assistant preparing an OpenEmbedded layer patch review.\nReview the provided Patch and select all potentially relevant guides from the index below.\nCRITICAL BIAS RULE: You MUST err on the side of inclusion. Only exclude a guide if it is 100% irrelevant to the modified files. If there is any doubt, include the file.\n\nYou MUST respond with ONLY a JSON object, no other text. Example:\n```json\n{\"selected_prompts\": [\"shell-tasks.md\"]}\n```",
        ))
        .user_prompt(
            PromptTemplate::<YoctoprojectPatchReviewState>::new(
                "<guide_index>\n@include(\"subsystem/subsystem.md\")\n</guide_index>\n\n<patch>\n{{target_commit_diff}}\n</patch>",
            )
            .with_var("target_commit_diff", |s: &YoctoprojectPatchReviewState| {
                s.target_commit_diff.clone()
            })
            .include_file("subsystem/subsystem.md"),
        )
        .output_format(OutputFormat::json_with_schema(json!({
            "type": "object",
            "properties": {
                "selected_prompts": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["selected_prompts"]
        })))
        .policy(StagePolicy {
            tools: ToolScope::None,
            max_turns: 1,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::lightweight_stage_effort),
            ..Default::default()
        })
        .skip_if(|s| s.manual_stages.is_some())
        .reduce(|state, out: PrescreenOutput| {
            state.selected_guides = out
                .selected_prompts
                .into_iter()
                .map(|name| normalize_guide_name(&name).to_string())
                .filter(|name| !is_stage_exclusive_guide(name))
                .filter(|name| sanitize_guide_name(name))
                .collect();
        })
        .build()
}

fn validate_planning_output_with_effort(
    out: &PlanningOutput,
    _state: &YoctoprojectPatchReviewState,
) -> Result<(), String> {
    validate_planning_stage_efforts(analysis_stage_by_name, out)
}

pub fn planning_stage_with_effort(
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<YoctoprojectPatchReviewState, PlanningOutput> {
    let dynamic_effort = effort == Some(crate::ai::EffortMode::Dynamic);
    let core_list = core_stage_list();
    let optional_list = ANALYSIS_STAGES
        .iter()
        .filter(|s| s.optional)
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(", ");

    let (sys_prompt, output_format) = if dynamic_effort {
        (
            format!(
                "You are an AI assistant planning an OpenEmbedded layer patch review.\nThe core stages ({core_list}) always run.\n1. Select which optional specialized stages should also run based on the patch contents.\nAvailable optional stages: [{optional_list}]\nInclude an optional stage when the patch touches that domain: recipe-metadata for any .bb, .bbappend, .inc, .bbclass or .conf change; patch-files for any .patch/.diff file or SRC_URI change; python for any Python in classes, recipes or lib/; reproducibility for build or install logic; security for CVE fixes, fetchers or installed permissions.\n\n2. Select the reasoning effort (\"medium\" or \"high\") in stage_efforts for each stage that will run (the core stages plus each stage selected in relevant_stages). Use \"high\" where the patch's main risk lies.\n\nRespond with ONLY a JSON object. Example:\n```json\n{{\"relevant_stages\": [\"recipe-metadata\", \"patch-files\"], \"stage_efforts\": {{\"goal\": \"medium\", \"implementation\": \"high\", \"commit-hygiene\": \"medium\", \"recipe-metadata\": \"high\", \"patch-files\": \"medium\"}}}}\n```"
            ),
            OutputFormat::json_with_schema(json!({
                "type": "object",
                "properties": {
                    "relevant_stages": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "stage_efforts": build_stage_efforts_schema(ANALYSIS_STAGES)
                },
                "required": ["relevant_stages", "stage_efforts"]
            }))
            .with_validator(validate_planning_output_with_effort)
            .with_feedback_formatter(format_planning_feedback),
        )
    } else {
        (
            format!(
                "You are an AI assistant planning an OpenEmbedded layer patch review.\nThe core stages ({core_list}) always run.\nSelect which optional specialized stages should also run based on the patch contents.\nAvailable optional stages: [{optional_list}]\nrecipe-metadata covers any .bb, .bbappend, .inc, .bbclass or .conf change; patch-files any .patch/.diff file or SRC_URI change; python any Python in classes, recipes or lib/; reproducibility build or install logic; security CVE fixes, fetchers or installed permissions.\n\nCRITICAL BIAS RULE: Err on the side of inclusion.\n\nRespond with ONLY a JSON object listing the relevant optional stages. Example:\n```json\n{{\"relevant_stages\": [\"recipe-metadata\", \"patch-files\"]}}\n```"
            ),
            OutputFormat::json_with_schema(json!({
                "type": "object",
                "properties": {
                    "relevant_stages": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["relevant_stages"]
            })),
        )
    };

    Stage::builder("planning")
        .system_prompt(PromptTemplate::<YoctoprojectPatchReviewState>::new(
            sys_prompt,
        ))
        .user_prompt(
            PromptTemplate::<YoctoprojectPatchReviewState>::new(
                "<patch>\n{{target_commit_diff}}\n</patch>",
            )
            .with_var("target_commit_diff", |s: &YoctoprojectPatchReviewState| {
                s.target_commit_diff.clone()
            }),
        )
        .output_format(output_format)
        .policy(StagePolicy {
            tools: ToolScope::None,
            max_turns: 1,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::lightweight_stage_effort),
            ..Default::default()
        })
        .skip_if(|s| s.cheap_mode || s.manual_stages.is_some())
        .reduce(move |state, out: PlanningOutput| {
            apply_planning_stage_output(
                ANALYSIS_STAGES,
                analysis_stage_by_name,
                dynamic_effort,
                out,
                &mut state.planned_stages,
                &mut state.planned_efforts,
            );
        })
        .build()
}

fn analysis_stage(
    def: &'static AnalysisStage,
    max_turns: usize,
    temperature: f32,
    reasoning_effort: Option<crate::ai::ReasoningEffort>,
) -> Box<dyn ExecutableStage<YoctoprojectPatchReviewState>> {
    let series_context = series_context_placeholder(def.wants_series_context);
    let mut user_template = PromptTemplate::<YoctoprojectPatchReviewState>::new(format!(
        "{}{}\n\n{}",
        def.instruction, series_context, CONCERN_JSON_SCHEMA_EXAMPLE
    ));
    for guide in def.guides {
        user_template = user_template.include_file(*guide);
    }
    let user_template = with_series_context(user_template, def.wants_series_context);

    Box::new(
        Stage::builder(def.name)
            .system_prompt(yoctoproject_system_prompt(def.uses_commit_log))
            .user_prompt(user_template)
            .output_format(
                OutputFormat::json()
                    .with_validator(validate_concerns_output)
                    .with_feedback_formatter(format_concerns_feedback),
            )
            .policy(StagePolicy {
                tools: ToolScope::All,
                max_turns,
                temperature,
                reasoning_effort,
                ..Default::default()
            })
            .reduce(
                move |state: &mut YoctoprojectPatchReviewState, out: StageConcernsOutput| {
                    let prompts = collect_stage_prompts(&state.selected_guides, def.guides);
                    append_stage_items_with_prompts(
                        &mut state.all_concerns,
                        &out.concerns,
                        def.name,
                        "General",
                        &prompts,
                    );
                    append_stage_dismissed_concerns_with_prompts(
                        &mut state.all_dismissed_concerns,
                        &out.dismissed_concerns,
                        def.name,
                        &prompts,
                    );
                },
            )
            .build(),
    )
}

/// Resolves the analysis stages to run: the manual selection, else the
/// planner's, else all of them. In cheap mode, or when the combined stage is
/// named, the selected stages run as one combined pass that shares the patch
/// and instructions instead of repeating them per stage.
pub fn resolve_analysis_stages_with_options(
    state: &YoctoprojectPatchReviewState,
    max_turns: usize,
    temperature: f32,
) -> Vec<Box<dyn ExecutableStage<YoctoprojectPatchReviewState>>> {
    let selected_stages: Vec<String> = if let Some(ref manual) = state.manual_stages {
        manual.clone()
    } else if !state.planned_stages.is_empty() {
        state.planned_stages.clone()
    } else {
        ANALYSIS_STAGES.iter().map(|d| d.name.to_string()).collect()
    };

    let mut include_prior_feedback = state.manual_stages.is_none();
    let mut use_combined = state.cheap_mode;
    let mut selected_defs: Vec<&'static AnalysisStage> = Vec::new();
    let mut select = |def: &'static AnalysisStage| {
        if !selected_defs.iter().any(|d| d.name == def.name) {
            selected_defs.push(def);
        }
    };
    for name in selected_stages {
        if crate::workflows::prior_feedback::is_prior_feedback_stage_name(&name) {
            include_prior_feedback = true;
            continue;
        }
        if normalize_stage_name(&name) == COMBINED_ANALYSIS_STAGE_NAME {
            use_combined = true;
            ANALYSIS_STAGES.iter().for_each(&mut select);
            continue;
        }
        match analysis_stage_by_name(&name) {
            Some(def) => select(def),
            None => tracing::warn!("Ignoring unknown Yocto Project review stage {:?}", name),
        }
    }

    let mut stages = Vec::new();
    if use_combined {
        if !selected_defs.is_empty() {
            stages.push(build_combined_analysis_stage(
                selected_defs,
                yoctoproject_system_prompt,
                CONCERN_JSON_SCHEMA_EXAMPLE,
                false,
                validate_concerns_output,
                format_concerns_feedback,
                max_turns,
                temperature,
                resolve_combined_stage_effort(state),
            ));
        }
    } else {
        for def in selected_defs {
            let effort = resolve_stage_effort(state, def.name);
            stages.push(analysis_stage(def, max_turns, temperature, effort));
        }
    }
    if include_prior_feedback {
        let prior_effort = resolve_stage_effort(
            state,
            crate::workflows::prior_feedback::PRIOR_FEEDBACK_STAGE_NAME,
        );
        stages.extend(
            crate::workflows::prior_feedback::build_prior_feedback_stages(
                state,
                || yoctoproject_system_prompt(true),
                max_turns,
                temperature,
                prior_effort,
            ),
        );
    }
    stages
}

/// Yocto Project inputs of the shared verification shard stage.
pub static YOCTOPROJECT_VERIFICATION_PROFILE:
    crate::workflows::verification_shards::VerificationShardProfile =
    crate::workflows::verification_shards::VerificationShardProfile {
        stage_name: "verification",
        instruction: STAGE_VERIFICATION_INSTRUCTION,
        problem_rule: r#""problem" (a short title under 80 characters starting with a recipe, class, module or file prefix like 'curl:', 'kernel.bbclass:', 'lib/oe/utils:', NEVER using backquotes)"#,
        example_output: "",
        wants_series_context: true,
        system_prompt: yoctoproject_system_prompt,
        stage_lookup: analysis_stage_by_name,
    };

pub fn resolve_verification_stages_with_options(
    state: &YoctoprojectPatchReviewState,
    max_turns: usize,
    temperature: f32,
) -> Vec<Box<dyn ExecutableStage<YoctoprojectPatchReviewState>>> {
    crate::workflows::verification_shards::resolve_verification_stages(
        &YOCTOPROJECT_VERIFICATION_PROFILE,
        state,
        max_turns,
        temperature,
    )
}

pub fn post_verification_stage_with_effort(
    stage_name: &'static str,
    batch: Vec<Value>,
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<YoctoprojectPatchReviewState, PostVerificationOutput> {
    let candidate_json =
        crate::workflows::linux_patch_review::render_items_with_untrusted_prior_context(&batch);
    let batch_for_prompts = batch.clone();
    let batch_for_validate = batch.clone();
    let batch_for_reduce = batch;
    let series_context = series_context_placeholder(POST_VERIFICATION.wants_series_context);
    let user_template = with_series_context(
        PromptTemplate::<YoctoprojectPatchReviewState>::new(format!(
            r#"{STAGE_POST_VERIFICATION_INSTRUCTION}

<false_positive_guide>
@include("false-positive-guide.md")
</false_positive_guide>

<severity_guidelines>
@include("severity.md")
</severity_guidelines>@includes

CRITICAL REVIEW DIRECTIVE: To dismiss a candidate, quote the disproving content in `dismissed_concerns[].locations`. If you cannot find concrete proof that the problem cannot happen, report it in `findings`.{series_context}

Inconclusive Concern(s) to Verify:
{{{{candidate_inconclusive_concerns}}}}

Return ONLY a JSON object with 'findings' and 'dismissed_concerns' arrays. Every candidate in this batch MUST be accounted for in either 'findings' (if validated) or 'dismissed_concerns' (if disproved; never return both empty arrays).
- LINEAGE REQUIREMENT ('source_ids'): Every inconclusive concern in this batch has an "id" ("I1", "I2", ...). Every object in 'findings' and 'dismissed_concerns' MUST include a non-empty "source_ids" array of strings listing the exact candidate "I*" ID(s) from this batch that it resolves, and every candidate "I*" ID in this batch MUST be accounted for in at least one output item's "source_ids".
- Each object in 'findings' MUST use: "source_ids" (non-empty array of batch "I*" IDs), "problem" (a short title under 80 characters starting with a recipe, class, module or file prefix like 'curl:', 'kernel.bbclass:', 'lib/oe/utils:', NEVER using backquotes), "severity" (Low, Medium, High, Critical, or Unknown), "severity_explanation" (detailed reasoning and proof), "preexisting" (boolean), "locations" (array of objects with file, function_or_symbol, line, code_snippet, side, and why_this_location_matters; side MUST be "PARENT" for deleted/pre-patch content or "REVISION" for added/modified/context content).
- Each object in 'dismissed_concerns' MUST use: "source_ids" (non-empty array of batch "I*" IDs), "description" (the candidate that was disproved), "reasoning" (step-by-step explanation of how the inspected content disproves it), and "locations" (a non-empty array of objects with file, function_or_symbol, line, code_snippet, side, and why_this_location_matters; side MUST be "PARENT" or "REVISION", quoting the verbatim disproving content)."#
        ))
        .include_file("false-positive-guide.md")
        .include_file("severity.md")
        .include_files_from_state(move |s: &YoctoprojectPatchReviewState| {
            extra_prompt_paths_for_items(&s.selected_guides, &batch_for_prompts, analysis_stage_by_name)
        }),
        POST_VERIFICATION.wants_series_context,
    )
    .with_var(
        "candidate_inconclusive_concerns",
        move |_: &YoctoprojectPatchReviewState| candidate_json.clone(),
    );

    Stage::builder(stage_name)
        .system_prompt(yoctoproject_system_prompt(true))
        .user_prompt(user_template)
        .output_format(
            OutputFormat::json()
                .with_validator(
                    move |out: &PostVerificationOutput, _: &YoctoprojectPatchReviewState| {
                        validate_post_verification_batch_items(out, &batch_for_validate)
                    },
                )
                .with_feedback_formatter(format_post_verification_feedback),
        )
        .policy(StagePolicy {
            tools: ToolScope::All,
            max_turns,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::verification_stage_effort),
            ..Default::default()
        })
        .reduce(move |state, out: PostVerificationOutput| {
            apply_post_verification_stage_output(
                state,
                stage_name,
                &batch_for_reduce,
                out,
                analysis_stage_by_name,
            );
        })
        .build()
}

pub fn resolve_post_verification_stages_with_options(
    state: &YoctoprojectPatchReviewState,
    max_turns: usize,
    temperature: f32,
) -> Vec<Box<dyn ExecutableStage<YoctoprojectPatchReviewState>>> {
    let effort = state.effort;
    batch_inconclusive_concerns_by_severity(&state.inconclusive_concerns)
        .into_iter()
        .enumerate()
        .map(|(idx, batch)| {
            Box::new(post_verification_stage_with_effort(
                POST_VERIFICATION_STAGE_NAMES[idx],
                batch,
                max_turns,
                temperature,
                effort,
            )) as Box<dyn ExecutableStage<YoctoprojectPatchReviewState>>
        })
        .collect()
}

pub fn report_stage_with_effort(
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<YoctoprojectPatchReviewState, String> {
    Stage::builder(REPORT.name)
        .system_prompt(yoctoproject_system_prompt(true))
        .user_prompt(
            PromptTemplate::<YoctoprojectPatchReviewState>::new(format!(
                r#"{STAGE_REPORT_INSTRUCTION}

<report_template>
@include("github-summary-template.md")
</report_template>

Findings:
{{{{findings}}}}

Return strictly plain text output (no markdown, no backticks, wrapped at 78 characters), not JSON."#
            ))
            .include_file("github-summary-template.md")
            .with_var("findings", |s: &YoctoprojectPatchReviewState| {
                crate::workflows::prior_attribution::report_findings_json(&s.findings)
            }),
        )
        .output_format(OutputFormat::text_with_validator(
            validate_github_summary_format,
            format_github_summary_feedback,
        ))
        .policy(StagePolicy {
            tools: ToolScope::All,
            max_turns,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::report_stage_effort),
            recitation_policy: RecitationPolicy::FallbackToFreeForm {
                reminder: "Do not quote large blocks of code verbatim. Summarize concisely."
                    .to_string(),
            },
            ..Default::default()
        })
        .skip_if(|s| s.skip_report || s.findings.is_empty())
        .reduce(|state, out: String| {
            state.review_inline = format_sashiko_inline_findings(&out);
        })
        .build()
}

pub fn summary_stage_with_effort(
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Stage<YoctoprojectPatchReviewState, String> {
    Stage::builder(SUMMARY.name)
        .system_prompt(yoctoproject_system_prompt(true))
        .user_prompt(PromptTemplate::<YoctoprojectPatchReviewState>::new(
            STAGE_SUMMARY_INSTRUCTION,
        ))
        .output_format(OutputFormat::text_with_validator(
            validate_summary_format,
            format_summary_feedback,
        ))
        .policy(StagePolicy {
            tools: ToolScope::None,
            max_turns: 1,
            temperature,
            reasoning_effort: effort.map(crate::ai::EffortMode::lightweight_stage_effort),
            recitation_policy: RecitationPolicy::FallbackToFreeForm {
                reminder: "Summarize the change concisely in plain text without quoting large blocks verbatim."
                    .to_string(),
            },
            ..Default::default()
        })
        .skip_if(|s| s.skip_report)
        .reduce(|state, out: String| {
            state.summary = out.trim().to_string();
        })
        .build()
}

// ---------------------------------------------------------------------------
// Complete Workflow Graph
// ---------------------------------------------------------------------------

/// Builds the Yocto Project patch review workflow.
///
/// The graph follows `build_sashiko_patch_review_workflow_with_effort`, with
/// the pre-screen first: the summary stage always runs, so empty stages are
/// skipped with `skip_if` rather than ending the workflow early, and a failed
/// verification shard aborts the review, since shards are the only path
/// from a candidate cluster to a finding.
pub fn build_yoctoproject_patch_review_workflow_with_effort(
    max_turns: usize,
    temperature: f32,
    effort: Option<crate::ai::EffortMode>,
) -> Workflow<YoctoprojectPatchReviewState> {
    Workflow::builder("yoctoproject_patch_review")
        .stage(prescreen_stage_with_effort(temperature, effort))
        .dynamic_parallel(
            planning_stage_with_effort(temperature, effort),
            move |state| resolve_analysis_stages_with_options(state, max_turns, temperature),
            ParallelPolicy::BestEffort,
        )
        .dynamic_parallel(
            deduplication_stage_with_effort(temperature, effort),
            move |state| resolve_verification_stages_with_options(state, max_turns, temperature),
            ParallelPolicy::FailFast,
        )
        .resolved_parallel(
            move |state| {
                resolve_post_verification_stages_with_options(state, max_turns, temperature)
            },
            ParallelPolicy::BestEffort,
        )
        .resolved_parallel(
            move |state| {
                crate::workflows::prior_attribution::resolve_prior_attribution_stages(
                    state,
                    temperature,
                )
            },
            ParallelPolicy::FailFast,
        )
        .stage(report_stage_with_effort(max_turns, temperature, effort))
        .stage(summary_stage_with_effort(temperature, effort))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_table() {
        let names: Vec<&str> = ANALYSIS_STAGES.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "goal",
                "implementation",
                "commit-hygiene",
                "recipe-metadata",
                "patch-files",
                "python",
                "reproducibility",
                "security",
            ]
        );
        assert_eq!(core_stage_list(), "goal, implementation, commit-hygiene");
        assert!(analysis_stage_by_name("Recipe_Metadata").unwrap().optional);
        assert_eq!(stage_short_label("patch_files"), Some("Patch Files"));
        assert_eq!(
            stage_short_label("post-verification-2"),
            Some("Post-Verification")
        );
    }

    #[test]
    fn test_consolidation_stage_names_resolve() {
        for name in ["deduplication", "report", "summary"] {
            assert!(consolidation_stage_by_name(name).is_some(), "{name}");
        }
        assert_eq!(
            consolidation_stage_by_name(VERIFICATION_STAGE_NAMES[1]).map(|s| s.name),
            Some("verification")
        );
        assert_eq!(stage_short_label("deduplication"), Some("Deduplication"));
        assert_eq!(
            stage_short_label(COMBINED_ANALYSIS_STAGE_NAME),
            Some(COMBINED_ANALYSIS_SHORT_LABEL)
        );
        assert_eq!(stage_short_label("pre-screen"), Some("Pre-Screen"));
    }

    fn analysis_stage_names(state: &YoctoprojectPatchReviewState) -> Vec<&'static str> {
        resolve_analysis_stages_with_options(state, 1, 0.0)
            .iter()
            .map(|stage| stage.name())
            .collect()
    }

    #[test]
    fn test_cheap_mode_runs_one_combined_analysis_stage() {
        let state = YoctoprojectPatchReviewState {
            manual_stages: Some(vec!["goal".into(), "patch-files".into()]),
            ..Default::default()
        };
        assert_eq!(analysis_stage_names(&state), vec!["goal", "patch-files"]);

        let cheap = YoctoprojectPatchReviewState {
            cheap_mode: true,
            ..state
        };
        assert_eq!(
            analysis_stage_names(&cheap),
            vec![COMBINED_ANALYSIS_STAGE_NAME]
        );
    }

    #[test]
    fn test_workflow_builds() {
        build_yoctoproject_patch_review_workflow_with_effort(1, 0.0, None);
    }

    #[test]
    fn test_stage_guides_exist_and_are_exclusive() {
        // A guide named here but missing from the prompt tree would be
        // included as nothing, so every stage would silently run without it.
        let root = std::path::Path::new("projects/yoctoproject/prompts");
        for def in ANALYSIS_STAGES {
            for guide in def.guides {
                assert!(root.join(guide).is_file(), "missing guide {guide}");
                let name = guide.rsplit('/').next().unwrap();
                assert!(is_stage_exclusive_guide(name), "{name}");
            }
        }
        assert!(!is_stage_exclusive_guide("shell-tasks.md"));
        for file in [
            "review-core.md",
            "severity.md",
            "false-positive-guide.md",
            "github-summary-template.md",
            "subsystem/subsystem.md",
            "patterns/shell-tasks.md",
        ] {
            assert!(root.join(file).is_file(), "missing {file}");
        }
    }

    #[test]
    fn test_system_prompt_names_openembedded_not_sashiko() {
        let state = YoctoprojectPatchReviewState::default();
        let rendered = yoctoproject_system_prompt(true).render_for_log(&state);
        assert!(rendered.contains("OpenEmbedded and Yocto Project maintainer"));
        assert!(!rendered.contains("Sashiko"), "{rendered}");
    }
}
