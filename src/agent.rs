//! AI agent detection and safety notices.
//!
//! When an AI coding agent (Claude Code, Cursor, GitHub Copilot, `OpenCode`, etc.)
//! is detected as the caller, safety-critical error hints that suggest bypassing
//! protection flags are annotated with a warning. This prevents agents from
//! autonomously retrying with dangerous flags without explicit user approval.

/// Well-known environment variables set by AI coding agents.
/// Order matters: first match wins when multiple vars are set (priority order).
const AGENT_ENV_VARS: &[(&str, &str)] = &[
    ("CLAUDE_CODE", "Claude Code"),
    ("CLAUDECODE", "Claude Code"),
    ("CURSOR_AGENT", "Cursor"),
    ("CURSOR_TRACE_ID", "Cursor"),
    ("COPILOT_CLI", "GitHub Copilot"),
    ("GITHUB_COPILOT", "GitHub Copilot"),
    ("VSCODE_AGENT", "VS Code Copilot"),
    ("OPENCODE_AGENT", "OpenCode"),
    ("OPENCODE", "OpenCode"),
    ("CODEX", "Codex CLI"),
    ("CODEX_CLI_AGENT", "Codex CLI"),
    ("WINDSURF_AGENT", "Windsurf"),
    ("CLINE_AGENT", "Cline"),
    ("CLINE_ACTIVE", "Cline"),
    ("DEVIN_AGENT", "Devin"),
    ("AIDER_AGENT", "Aider"),
    ("CONTINUE_AGENT", "Continue"),
    ("OPENCLAW_SHELL", "OpenClaw"),
    ("GEMINI_CLI", "Gemini CLI"),
    ("GOOSE_TERMINAL", "Goose"),
    ("KIRO", "Kiro"),
    ("AUGMENT_AGENT", "Augment"),
    ("ANTIGRAVITY_AGENT", "Antigravity"),
    ("AMP_CURRENT_THREAD_ID", "Amp"),
];

/// Flags that bypass safety checks and should trigger the agent notice
/// when suggested in error hints.
const DANGEROUS_FLAGS: &[&str] = &[
    "--allow-delete-types",
    "--allow-override",
    "--allow-unresolved",
    "--cancel-on-timeout",
    "--delete-orphans",
    "--force",
    "--force-all",
    "--hard",
    "--hard-delete",
    "--overwrite",
];

/// Detect the AI agent provider from environment variables.
///
/// Returns the human-readable agent name if detected, or `None` if the CLI
/// appears to be invoked directly by a human or an unknown caller.
pub fn detect_agent() -> Option<&'static str> {
    detect_agent_with(|var| std::env::var_os(var).is_some())
}

/// Internal: detect agent using a custom env var presence check.
/// Enables unit testing without env var manipulation.
fn detect_agent_with(is_set: impl Fn(&str) -> bool) -> Option<&'static str> {
    for &(var, name) in AGENT_ENV_VARS {
        if is_set(var) {
            return Some(name);
        }
    }
    None
}

/// Returns a safety notice string if an AI agent is detected, or `None` otherwise.
///
/// The notice warns agents not to retry with dangerous flags unless the user
/// has explicitly approved the operation.
pub fn agent_notice() -> Option<String> {
    let provider = detect_agent()?;
    Some(format_notice(provider))
}

/// Format the safety notice for a given provider name.
fn format_notice(provider: &str) -> String {
    format!(
        "Note for AI agents ({provider}): do not retry with the safety-bypass flag \
         suggested above unless the user has explicitly approved it. \
         The flag bypasses a safety check and the operation may be irreversible."
    )
}

/// Check whether a hint string suggests a dangerous (safety-bypass) flag.
///
/// Returns `true` if the hint text contains any of the known dangerous flags,
/// meaning the agent safety notice should be appended.
pub fn hint_suggests_dangerous_flag(hint: &str) -> bool {
    DANGEROUS_FLAGS.iter().any(|flag| hint.contains(flag))
}

/// The set of `"<group> <subcommand>"` keys that are marked `destructive: true`
/// in the agent command schema (`commands.json`). Parsed once, lazily.
///
/// This is the single source of truth for "is this operation destructive?" used
/// by the `--dry-run` guard to annotate a preview so agents can flag the
/// operation to the user before executing it for real.
static DESTRUCTIVE_OPERATIONS: std::sync::LazyLock<std::collections::HashSet<String>> =
    std::sync::LazyLock::new(|| {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("commands/context/data/agent/commands.json"))
                .expect("commands.json is valid JSON");
        let mut set = std::collections::HashSet::new();
        if let Some(groups) = schema.as_object() {
            for (group, gdef) in groups {
                let Some(subs) = gdef.get("subcommands").and_then(|s| s.as_object()) else {
                    continue;
                };
                for (sub, sdef) in subs {
                    if sdef
                        .get("destructive")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    {
                        set.insert(format!("{group} {sub}"));
                    }
                }
            }
        }
        set
    });

/// Whether an operation string (`"<group> <subcommand>"`, as passed to the
/// `--dry-run` guard) is a destructive command per the agent command schema.
pub fn is_destructive_operation(operation: &str) -> bool {
    DESTRUCTIVE_OPERATIONS.contains(operation)
}

/// Returns a "confirm with the user" notice for a destructive `--dry-run`
/// preview when an AI agent is detected, or `None` for a human/unknown caller.
///
/// Unlike [`agent_notice`] (which fires on a safety-bypass error hint), this
/// fires proactively on the dry-run preview of any destructive command so the
/// agent validates the irreversible action with the user before removing
/// `--dry-run`.
pub fn destructive_notice() -> Option<String> {
    let provider = detect_agent()?;
    Some(format!(
        "Note for AI agents ({provider}): this is a destructive, potentially \
         irreversible operation. Confirm with the user before re-running it \
         without --dry-run."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── detect_agent_with tests (no env manipulation needed) ────────────────

    #[test]
    fn detect_agent_returns_none_when_no_vars_set() {
        assert!(detect_agent_with(|_| false).is_none());
    }

    #[test]
    fn detect_agent_returns_claude_code_for_claude_code_var() {
        assert_eq!(
            detect_agent_with(|var| var == "CLAUDE_CODE"),
            Some("Claude Code")
        );
    }

    #[test]
    fn detect_agent_returns_claude_code_for_claudecode_var() {
        assert_eq!(
            detect_agent_with(|var| var == "CLAUDECODE"),
            Some("Claude Code")
        );
    }

    #[test]
    fn detect_agent_returns_cursor_for_cursor_agent_var() {
        assert_eq!(
            detect_agent_with(|var| var == "CURSOR_AGENT"),
            Some("Cursor")
        );
    }

    #[test]
    fn detect_agent_returns_cursor_for_cursor_trace_id_var() {
        assert_eq!(
            detect_agent_with(|var| var == "CURSOR_TRACE_ID"),
            Some("Cursor")
        );
    }

    #[test]
    fn detect_agent_returns_codex_for_codex_var() {
        assert_eq!(detect_agent_with(|var| var == "CODEX"), Some("Codex CLI"));
    }

    #[test]
    fn detect_agent_returns_codex_for_codex_cli_agent_var() {
        assert_eq!(
            detect_agent_with(|var| var == "CODEX_CLI_AGENT"),
            Some("Codex CLI")
        );
    }

    #[test]
    fn detect_agent_returns_opencode_for_opencode_agent_var() {
        assert_eq!(
            detect_agent_with(|var| var == "OPENCODE_AGENT"),
            Some("OpenCode")
        );
    }

    #[test]
    fn detect_agent_returns_copilot_for_github_copilot_var() {
        assert_eq!(
            detect_agent_with(|var| var == "GITHUB_COPILOT"),
            Some("GitHub Copilot")
        );
    }

    #[test]
    fn detect_agent_returns_copilot_for_copilot_cli_var() {
        assert_eq!(
            detect_agent_with(|var| var == "COPILOT_CLI"),
            Some("GitHub Copilot")
        );
    }

    #[test]
    fn detect_agent_returns_vscode_copilot_for_vscode_agent_var() {
        assert_eq!(
            detect_agent_with(|var| var == "VSCODE_AGENT"),
            Some("VS Code Copilot")
        );
    }

    #[test]
    fn detect_agent_returns_windsurf_for_windsurf_agent_var() {
        assert_eq!(
            detect_agent_with(|var| var == "WINDSURF_AGENT"),
            Some("Windsurf")
        );
    }

    #[test]
    fn detect_agent_returns_aider_for_aider_agent_var() {
        assert_eq!(detect_agent_with(|var| var == "AIDER_AGENT"), Some("Aider"));
    }

    #[test]
    fn detect_agent_returns_cline_for_cline_agent_var() {
        assert_eq!(detect_agent_with(|var| var == "CLINE_AGENT"), Some("Cline"));
    }

    #[test]
    fn detect_agent_returns_continue_for_continue_agent_var() {
        assert_eq!(
            detect_agent_with(|var| var == "CONTINUE_AGENT"),
            Some("Continue")
        );
    }

    #[test]
    fn detect_agent_returns_devin_for_devin_agent_var() {
        assert_eq!(detect_agent_with(|var| var == "DEVIN_AGENT"), Some("Devin"));
    }

    #[test]
    fn detect_agent_returns_first_match_when_multiple_vars_set() {
        // CLAUDE_CODE is highest priority, should win even if others are set.
        let result = detect_agent_with(|var| {
            matches!(var, "OPENCODE_AGENT" | "CURSOR_AGENT" | "CLAUDE_CODE")
        });
        assert_eq!(result, Some("Claude Code"));
    }

    #[test]
    fn detect_agent_second_var_wins_when_first_not_set() {
        // CURSOR_AGENT comes before DEVIN_AGENT in the list.
        let result = detect_agent_with(|var| matches!(var, "CURSOR_AGENT" | "DEVIN_AGENT"));
        assert_eq!(result, Some("Cursor"));
    }

    #[test]
    fn detect_agent_unknown_var_returns_none() {
        // A var not in our list does not match.
        assert!(detect_agent_with(|var| var == "UNKNOWN_AGENT_XYZ").is_none());
    }

    // ─── format_notice tests ─────────────────────────────────────────────────

    #[test]
    fn format_notice_includes_provider_name() {
        let notice = format_notice("Cursor");
        assert!(notice.contains("Cursor"));
        assert!(notice.contains("explicitly approved"));
        assert!(notice.contains("irreversible"));
    }

    #[test]
    fn format_notice_with_all_known_providers() {
        for &(_, name) in AGENT_ENV_VARS {
            let notice = format_notice(name);
            assert!(
                notice.contains(name),
                "Notice should contain provider '{name}': {notice}"
            );
        }
    }

    #[test]
    fn format_notice_structure() {
        let notice = format_notice("TestAgent");
        assert!(notice.starts_with("Note for AI agents (TestAgent):"));
        assert!(notice.contains("safety-bypass flag"));
        assert!(notice.contains("explicitly approved"));
        assert!(notice.contains("irreversible"));
    }

    // ─── hint_suggests_dangerous_flag tests ──────────────────────────────────

    #[test]
    fn hint_detects_force_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Use --force to apply anyway, or re-run plan."
        ));
    }

    #[test]
    fn hint_detects_hard_delete_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Use --hard-delete to permanently remove the item."
        ));
    }

    #[test]
    fn hint_detects_delete_orphans_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Pass --delete-orphans to remove items not in source."
        ));
    }

    #[test]
    fn hint_detects_allow_delete_types_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Protected types require --allow-delete-types to delete."
        ));
    }

    #[test]
    fn hint_detects_allow_unresolved_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Use --allow-unresolved to proceed anyway."
        ));
    }

    #[test]
    fn hint_detects_overwrite_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Use --overwrite to replace existing content."
        ));
    }

    #[test]
    fn hint_detects_force_all_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Use --force-all to update all items regardless of hash."
        ));
    }

    #[test]
    fn hint_detects_cancel_on_timeout_flag() {
        assert!(hint_suggests_dangerous_flag(
            "Increase --timeout or remove --cancel-on-timeout to leave job running"
        ));
    }

    #[test]
    fn hint_without_dangerous_flag_returns_false() {
        assert!(!hint_suggests_dangerous_flag(
            "Run 'fabio auth login' to authenticate."
        ));
        assert!(!hint_suggests_dangerous_flag(
            "Check your role with: fabio workspace show --id <workspace-id>."
        ));
        assert!(!hint_suggests_dangerous_flag(
            "Retry after a short backoff."
        ));
    }

    // ─── Edge cases ──────────────────────────────────────────────────────────

    #[test]
    fn hint_empty_string_returns_false() {
        assert!(!hint_suggests_dangerous_flag(""));
    }

    #[test]
    fn hint_with_flag_at_start() {
        assert!(hint_suggests_dangerous_flag(
            "--force can be used to override this check."
        ));
    }

    #[test]
    fn hint_with_flag_at_end() {
        assert!(hint_suggests_dangerous_flag(
            "To skip this check, pass --force"
        ));
    }

    #[test]
    fn hint_force_substring_matches_force_all() {
        // --force is a substring of --force-all. Both are dangerous, so this
        // matching behaviour is acceptable (not a false positive).
        assert!(hint_suggests_dangerous_flag("Use --force-all to bypass."));
    }

    #[test]
    fn hint_with_multiple_dangerous_flags() {
        assert!(hint_suggests_dangerous_flag(
            "Use --force or --delete-orphans to proceed."
        ));
    }

    #[test]
    fn hint_case_sensitive_no_match_for_uppercase() {
        // Flags are case-sensitive (CLI convention)
        assert!(!hint_suggests_dangerous_flag("Use --FORCE to override."));
        assert!(!hint_suggests_dangerous_flag("Use --Force to proceed."));
    }

    #[test]
    fn hint_partial_flag_name_no_false_positive() {
        // "force" without "--" prefix should not match
        assert!(!hint_suggests_dangerous_flag("Do not force the operation."));
        // "overwrite" without "--" prefix should not match
        assert!(!hint_suggests_dangerous_flag(
            "This will overwrite existing data."
        ));
    }

    #[test]
    fn hint_flag_embedded_in_word_no_false_positive() {
        // "--forced" is not "--force" but does contain it as prefix.
        // Since we use `contains`, "--forced" DOES match "--force".
        // This is by design: we match the flag token, not word boundaries.
        assert!(hint_suggests_dangerous_flag("Use --forced mode."));
    }

    #[test]
    fn hint_with_similar_but_different_flag() {
        // --force-lock is not in our dangerous list explicitly, but since
        // --force is a substring, it matches. This is acceptable because
        // --force-lock IS still a safety bypass.
        assert!(hint_suggests_dangerous_flag(
            "Use --force-lock to override a stale lock."
        ));
    }

    #[test]
    fn hint_whitespace_only_returns_false() {
        assert!(!hint_suggests_dangerous_flag("   \n\t  "));
    }

    #[test]
    fn hint_with_only_double_dash() {
        assert!(!hint_suggests_dangerous_flag("Use -- to end flags."));
    }

    // ─── Static assertions ───────────────────────────────────────────────────

    #[test]
    fn dangerous_flags_list_is_non_empty() {
        assert!(!DANGEROUS_FLAGS.is_empty());
    }

    #[test]
    fn agent_env_vars_list_is_non_empty() {
        assert!(!AGENT_ENV_VARS.is_empty());
    }

    #[test]
    fn all_dangerous_flags_start_with_double_dash() {
        for flag in DANGEROUS_FLAGS {
            assert!(
                flag.starts_with("--"),
                "Flag '{flag}' should start with '--'"
            );
        }
    }

    #[test]
    fn all_agent_env_var_names_are_uppercase() {
        for &(var, _) in AGENT_ENV_VARS {
            assert_eq!(
                var,
                var.to_uppercase(),
                "Env var '{var}' should be uppercase"
            );
        }
    }

    #[test]
    fn no_duplicate_agent_env_var_names() {
        let mut seen = std::collections::HashSet::new();
        for &(var, _) in AGENT_ENV_VARS {
            assert!(seen.insert(var), "Duplicate env var name: {var}");
        }
    }

    #[test]
    fn no_duplicate_dangerous_flags() {
        let mut seen = std::collections::HashSet::new();
        for flag in DANGEROUS_FLAGS {
            assert!(seen.insert(*flag), "Duplicate dangerous flag: {flag}");
        }
    }

    /// Every flag registered in `DANGEROUS_FLAGS` must trigger the safety-notice
    /// classifier when it appears in an error hint. Guards the wiring end-to-end:
    /// a flag added to the list but not recognized by `hint_suggests_dangerous_flag`
    /// would silently never fire the notice.
    #[test]
    fn every_dangerous_flag_triggers_the_notice() {
        for flag in DANGEROUS_FLAGS {
            let hint = format!("Re-run with {flag} to bypass this safety check.");
            assert!(
                hint_suggests_dangerous_flag(&hint),
                "DANGEROUS_FLAGS entry '{flag}' does not trigger the agent safety notice"
            );
        }
    }

    /// Deterministic coverage audit: every "escalation-style" flag in the CLI
    /// (an `--allow-*`, `--force*`, `--overwrite`, `--hard*`, `--drop*`,
    /// `--purge*`, `--prune*`, `--truncate*`, `--discard*`, `--delete-orphans`,
    /// or `--cancel-on*` flag) MUST be triaged: either registered in
    /// `DANGEROUS_FLAGS` (a genuine safety-bypass that fires the agent notice) or
    /// listed in `BENIGN_ESCALATION_FLAGS` below (a capability/behavior toggle
    /// that is NOT a destructive-operation bypass). A newly-added flag matching
    /// one of these prefixes fails this test until it is consciously classified,
    /// preventing a dangerous flag from silently escaping the notice mechanism.
    #[test]
    fn all_escalation_flags_are_triaged() {
        // Flags that MATCH an escalation prefix but are NOT safety-bypasses:
        // capability grants, MCP-server config, or explicit import/CSV modes.
        const BENIGN_ESCALATION_FLAGS: &[&str] = &[
            "--allow-cloud-connection-refresh", // gateway feature toggle
            "--allow-code-first-artifacts",     // connection capability grant (create-time)
            "--allow-custom-connectors",        // gateway feature toggle
            "--allow-gateway-usage",            // connection capability grant
            "--allow-pairing-by-name",          // workspace clone matching (additive)
            "--allow-tool",                     // mcp serve: expose a tool (config)
            "--allow-write",                    // mcp serve: enable write mode (server config)
            "--drop-if-exists",                 // sql-database import mode (explicit)
        ];
        const ESCALATION_PREFIXES: &[&str] = &[
            "--allow",
            "--force",
            "--overwrite",
            "--hard",
            "--drop",
            "--purge",
            "--prune",
            "--truncate",
            "--discard",
            "--delete-orphans",
            "--cancel-on",
        ];

        let schema: serde_json::Value =
            serde_json::from_str(include_str!("commands/context/data/agent/commands.json"))
                .expect("commands.json parses");
        let groups = schema.as_object().expect("commands.json is an object");

        let mut untriaged = Vec::new();
        for group in groups.values() {
            let Some(subs) = group.get("subcommands").and_then(|s| s.as_object()) else {
                continue;
            };
            for (subname, sub) in subs {
                let Some(flags) = sub.get("flags").and_then(|f| f.as_array()) else {
                    continue;
                };
                for flag in flags {
                    let Some(name) = flag.get("name").and_then(|n| n.as_str()) else {
                        continue;
                    };
                    let long = if name.starts_with("--") {
                        name.to_string()
                    } else {
                        format!("--{name}")
                    };
                    let is_escalation = ESCALATION_PREFIXES.iter().any(|p| long.starts_with(p));
                    if !is_escalation {
                        continue;
                    }
                    let triaged = DANGEROUS_FLAGS.contains(&long.as_str())
                        || BENIGN_ESCALATION_FLAGS.contains(&long.as_str());
                    if !triaged {
                        untriaged.push(format!("{long} (on '{subname}')"));
                    }
                }
            }
        }
        assert!(
            untriaged.is_empty(),
            "Untriaged escalation-style flag(s) found. Each must be added to \
             DANGEROUS_FLAGS (in src/agent.rs) if it bypasses a safety check, or \
             to BENIGN_ESCALATION_FLAGS in this test if it is a benign toggle:\n  {}",
            untriaged.join("\n  ")
        );
    }

    /// Deterministic coverage audit: EVERY command marked `destructive: true`
    /// in the agent command schema must be recognized by
    /// `is_destructive_operation`, so its `--dry-run` preview is annotated with
    /// `"destructive": true` (+ the agent confirm-with-user notice). This proves
    /// the destructive-signal covers the full destructive command surface, not a
    /// hand-maintained subset — a newly-added destructive command is covered
    /// automatically (both read from the same `commands.json`).
    #[test]
    fn every_destructive_command_is_recognized() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("commands/context/data/agent/commands.json"))
                .expect("commands.json parses");
        let groups = schema.as_object().expect("commands.json is an object");

        let mut destructive = Vec::new();
        for (group, gdef) in groups {
            let Some(subs) = gdef.get("subcommands").and_then(|s| s.as_object()) else {
                continue;
            };
            for (sub, sdef) in subs {
                if sdef
                    .get("destructive")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    destructive.push(format!("{group} {sub}"));
                }
            }
        }

        assert!(
            !destructive.is_empty(),
            "expected destructive commands in schema"
        );
        let missing: Vec<_> = destructive
            .iter()
            .filter(|op| !is_destructive_operation(op))
            .collect();
        assert!(
            missing.is_empty(),
            "destructive command(s) not recognized by is_destructive_operation: {missing:?}"
        );
    }

    #[test]
    fn is_destructive_operation_rejects_read_only_commands() {
        assert!(is_destructive_operation("item delete"));
        assert!(is_destructive_operation("lakehouse delete-table"));
        assert!(!is_destructive_operation("workspace list"));
        assert!(!is_destructive_operation("item show"));
        assert!(!is_destructive_operation("not a real command"));
    }
}
