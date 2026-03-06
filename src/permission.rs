use anyhow::Result;
use regex::Regex;
use std::fmt;

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub enum Permission {
    Allow,
    AskOnce,
    #[default]
    Ask,
    Deny,
    /// Combined rules: check blacklist → whitelist → fallback mode.
    ///
    /// Evaluation order:
    ///   1. If blacklist matches → Deny
    ///   2. If whitelist is present and matches → Allow
    ///   3. If whitelist is present but doesn't match → Deny
    ///   4. Otherwise → fallback to `mode` (or tool default if mode is empty)
    Rules {
        blacklist: Vec<String>,
        whitelist: Vec<String>,
        /// Fallback mode when neither list matches.
        /// Empty string means "use the tool's built-in default".
        mode: String,
    },
}

#[allow(dead_code)]
impl Permission {
    /// Parse a simple mode string into a Permission.
    /// Only accepts: "allow", "ask", "ask_once", "deny".
    pub fn parse(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        match trimmed {
            "allow" => Ok(Permission::Allow),
            "ask_once" => Ok(Permission::AskOnce),
            "ask" => Ok(Permission::Ask),
            "deny" => Ok(Permission::Deny),
            _ => anyhow::bail!(
                "Invalid permission: '{}'. Expected: allow, deny, ask, ask_once",
                trimmed
            ),
        }
    }

    pub fn restrictiveness(&self) -> u8 {
        match self {
            Permission::Allow => 0,
            Permission::AskOnce => 1,
            Permission::Ask => 2,
            Permission::Rules { .. } => 3,
            Permission::Deny => 4,
        }
    }

    pub fn is_more_restrictive_than(&self, other: &Permission) -> bool {
        self.restrictiveness() > other.restrictiveness()
    }

    /// Merge a task-level override into this permission.
    /// Task can only tighten (more restrictive), never relax.
    pub fn merge_override(&self, task_override: &Permission) -> Permission {
        if task_override.is_more_restrictive_than(self) {
            task_override.clone()
        } else {
            self.clone()
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::Allow => write!(f, "allow"),
            Permission::AskOnce => write!(f, "ask_once"),
            Permission::Ask => write!(f, "ask"),
            Permission::Deny => write!(f, "deny"),
            Permission::Rules {
                blacklist,
                whitelist,
                mode,
            } => {
                let mut parts = Vec::new();
                if !blacklist.is_empty() {
                    parts.push(format!("blacklist: [{}]", blacklist.join(", ")));
                }
                if !whitelist.is_empty() {
                    parts.push(format!("whitelist: [{}]", whitelist.join(", ")));
                }
                if !mode.is_empty() {
                    parts.push(format!("mode: {}", mode));
                }
                write!(f, "{{ {} }}", parts.join(", "))
            }
        }
    }
}

/// Check a command string against the permission policy.
pub enum PermissionDecision {
    Allow,
    Deny(String),
    NeedAsk,
}

pub fn check_permission(permission: &Permission, command: &str) -> PermissionDecision {
    match permission {
        Permission::Allow => PermissionDecision::Allow,
        Permission::Deny => PermissionDecision::Deny("tool is disabled".to_string()),
        Permission::Ask | Permission::AskOnce => PermissionDecision::NeedAsk,
        Permission::Rules {
            blacklist,
            whitelist,
            mode,
        } => {
            // 1. Check blacklist — if any pattern matches, deny.
            for pattern in blacklist {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(command) {
                        return PermissionDecision::Deny(format!(
                            "matched blacklist pattern: {}",
                            pattern
                        ));
                    }
                }
            }

            // 2. Check whitelist — if present and matches, allow.
            if !whitelist.is_empty() {
                for pattern in whitelist {
                    if let Ok(re) = Regex::new(pattern) {
                        if re.is_match(command) {
                            return PermissionDecision::Allow;
                        }
                    }
                }
                // Whitelist present but no match → deny.
                return PermissionDecision::Deny("not in whitelist".to_string());
            }

            // 3. Fallback to mode.
            if mode.is_empty() {
                // No mode specified — use tool default (Ask).
                PermissionDecision::NeedAsk
            } else {
                match mode.as_str() {
                    "allow" => PermissionDecision::Allow,
                    "deny" => PermissionDecision::Deny("tool is disabled".to_string()),
                    _ => PermissionDecision::NeedAsk,
                }
            }
        }
    }
}

pub fn check_user_blocklist(command: &str, user_patterns: &[String]) -> Option<String> {
    for pattern in user_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(command) {
                return Some(format!("blocked by user safety rule: {}", pattern));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_permission() {
        assert!(matches!(
            Permission::parse("allow").unwrap(),
            Permission::Allow
        ));
        assert!(matches!(
            Permission::parse("deny").unwrap(),
            Permission::Deny
        ));
        assert!(matches!(Permission::parse("ask").unwrap(), Permission::Ask));
        assert!(matches!(
            Permission::parse("ask_once").unwrap(),
            Permission::AskOnce
        ));
    }

    #[test]
    fn test_parse_rejects_old_formats() {
        // Old pipe-delimited blacklist/whitelist strings are no longer supported.
        assert!(Permission::parse("blacklist: rm|shutdown").is_err());
        assert!(Permission::parse("whitelist: ^curl|^wget").is_err());
    }

    #[test]
    fn test_rules_blacklist_denies() {
        let perm = Permission::Rules {
            blacklist: vec![r"rm\s+-rf".to_string(), "shutdown".to_string()],
            whitelist: vec![],
            mode: String::new(),
        };
        assert!(matches!(
            check_permission(&perm, "ls -la"),
            PermissionDecision::NeedAsk
        ));
        assert!(matches!(
            check_permission(&perm, "rm -rf /tmp"),
            PermissionDecision::Deny(_)
        ));
        assert!(matches!(
            check_permission(&perm, "shutdown now"),
            PermissionDecision::Deny(_)
        ));
    }

    #[test]
    fn test_rules_whitelist_allows_and_denies() {
        let perm = Permission::Rules {
            blacklist: vec![],
            whitelist: vec![r"^cargo ".to_string(), r"^npm ".to_string()],
            mode: String::new(),
        };
        assert!(matches!(
            check_permission(&perm, "cargo build"),
            PermissionDecision::Allow
        ));
        assert!(matches!(
            check_permission(&perm, "npm install"),
            PermissionDecision::Allow
        ));
        assert!(matches!(
            check_permission(&perm, "rm -rf /"),
            PermissionDecision::Deny(_)
        ));
    }

    #[test]
    fn test_rules_blacklist_takes_precedence_over_whitelist() {
        let perm = Permission::Rules {
            blacklist: vec![r"--force".to_string()],
            whitelist: vec![r"^git ".to_string()],
            mode: String::new(),
        };
        // Whitelisted but also blacklisted → deny wins.
        assert!(matches!(
            check_permission(&perm, "git push --force"),
            PermissionDecision::Deny(_)
        ));
        // Whitelisted and not blacklisted → allow.
        assert!(matches!(
            check_permission(&perm, "git commit -m 'msg'"),
            PermissionDecision::Allow
        ));
    }

    #[test]
    fn test_rules_fallback_mode() {
        let perm = Permission::Rules {
            blacklist: vec!["sudo".to_string()],
            whitelist: vec![],
            mode: "allow".to_string(),
        };
        // No blacklist match, no whitelist → fallback to "allow".
        assert!(matches!(
            check_permission(&perm, "ls -la"),
            PermissionDecision::Allow
        ));
        // Blacklist match → deny regardless of fallback.
        assert!(matches!(
            check_permission(&perm, "sudo rm"),
            PermissionDecision::Deny(_)
        ));
    }

    #[test]
    fn test_rules_empty_mode_defaults_to_ask() {
        let perm = Permission::Rules {
            blacklist: vec![],
            whitelist: vec![],
            mode: String::new(),
        };
        assert!(matches!(
            check_permission(&perm, "anything"),
            PermissionDecision::NeedAsk
        ));
    }

    #[test]
    fn test_restrictiveness_order() {
        assert!(Permission::Deny.is_more_restrictive_than(&Permission::Allow));
        assert!(Permission::Ask.is_more_restrictive_than(&Permission::AskOnce));
        assert!(!Permission::Allow.is_more_restrictive_than(&Permission::Deny));
    }

    #[test]
    fn test_user_blocklist() {
        let user = vec!["DROP\\s+TABLE".to_string()];
        assert!(check_user_blocklist("DROP TABLE users", &user).is_some());
        assert!(check_user_blocklist("SELECT * FROM users", &user).is_none());
    }

    #[test]
    fn test_user_blocklist_empty() {
        assert!(check_user_blocklist("rm -rf /", &[]).is_none());
        assert!(check_user_blocklist("anything", &[]).is_none());
    }

    #[test]
    fn test_merge_cannot_relax() {
        let global = Permission::Ask;
        let task = Permission::Allow;
        let merged = global.merge_override(&task);
        assert!(matches!(merged, Permission::Ask));
    }

    #[test]
    fn test_merge_can_restrict() {
        let global = Permission::Allow;
        let task = Permission::Ask;
        let merged = global.merge_override(&task);
        assert!(matches!(merged, Permission::Ask));
    }
}
