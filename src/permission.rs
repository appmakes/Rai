use anyhow::Result;
use regex::Regex;
use std::fmt;

#[derive(Debug, Clone, Default)]
pub enum Permission {
    Allow,
    Blacklist(Vec<String>),
    AskOnce,
    #[default]
    Ask,
    Whitelist(Vec<String>),
    Deny,
}

impl Permission {
    pub fn parse(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        match trimmed {
            "allow" => Ok(Permission::Allow),
            "ask_once" => Ok(Permission::AskOnce),
            "ask" => Ok(Permission::Ask),
            "deny" => Ok(Permission::Deny),
            _ if trimmed.starts_with("blacklist:") => {
                let patterns = trimmed["blacklist:".len()..]
                    .trim()
                    .split('|')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                Ok(Permission::Blacklist(patterns))
            }
            _ if trimmed.starts_with("whitelist:") => {
                let patterns = trimmed["whitelist:".len()..]
                    .trim()
                    .split('|')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                Ok(Permission::Whitelist(patterns))
            }
            _ => anyhow::bail!("Invalid permission: '{}'. Expected: allow, deny, ask, ask_once, blacklist:<patterns>, whitelist:<patterns>", trimmed),
        }
    }

    pub fn restrictiveness(&self) -> u8 {
        match self {
            Permission::Allow => 0,
            Permission::Blacklist(_) => 1,
            Permission::AskOnce => 2,
            Permission::Ask => 3,
            Permission::Whitelist(_) => 4,
            Permission::Deny => 5,
        }
    }

    pub fn is_more_restrictive_than(&self, other: &Permission) -> bool {
        self.restrictiveness() > other.restrictiveness()
    }

    /// Merge a task-level override into this permission.
    /// Task can only tighten (more restrictive), never relax.
    /// Blacklist patterns are unioned, whitelist patterns are intersected.
    pub fn merge_override(&self, task_override: &Permission) -> Permission {
        if !task_override.is_more_restrictive_than(self) {
            match (self, task_override) {
                (Permission::Blacklist(global), Permission::Blacklist(task)) => {
                    let mut merged = global.clone();
                    for p in task {
                        if !merged.contains(p) {
                            merged.push(p.clone());
                        }
                    }
                    return Permission::Blacklist(merged);
                }
                (Permission::Whitelist(global), Permission::Whitelist(task)) => {
                    let merged: Vec<String> = global
                        .iter()
                        .filter(|p| task.contains(p))
                        .cloned()
                        .collect();
                    return Permission::Whitelist(merged);
                }
                _ => return self.clone(),
            }
        }
        task_override.clone()
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::Allow => write!(f, "allow"),
            Permission::Blacklist(p) => write!(f, "blacklist: {}", p.join("|")),
            Permission::AskOnce => write!(f, "ask_once"),
            Permission::Ask => write!(f, "ask"),
            Permission::Whitelist(p) => write!(f, "whitelist: {}", p.join("|")),
            Permission::Deny => write!(f, "deny"),
        }
    }
}

/// Check a command string against the permission policy.
/// Returns Ok(true) if allowed, Ok(false) if denied, Err if needs user input.
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
        Permission::Blacklist(patterns) => {
            for pattern in patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(command) {
                        return PermissionDecision::Deny(format!(
                            "matched blacklist pattern: {}",
                            pattern
                        ));
                    }
                }
            }
            PermissionDecision::Allow
        }
        Permission::Whitelist(patterns) => {
            for pattern in patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(command) {
                        return PermissionDecision::Allow;
                    }
                }
            }
            PermissionDecision::Deny("not in whitelist".to_string())
        }
    }
}

const HARDCODED_BLOCKLIST: &[&str] = &[
    r"rm\s+-rf\s+/[^\.]",
    r"rm\s+-rf\s+/$",
    r"mkfs\.",
    r"dd\s+if=.*of=/dev/",
    r":\(\)\{.*\|.*&\s*\};",
    r">\s*/dev/sd",
    r"chmod\s+-R\s+777\s+/",
    r"shutdown",
    r"reboot",
];

pub fn check_global_blocklist(command: &str, user_patterns: &[String]) -> Option<String> {
    for pattern in HARDCODED_BLOCKLIST {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(command) {
                return Some(format!("blocked by safety rule: {}", pattern));
            }
        }
    }
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
        assert!(matches!(Permission::parse("allow").unwrap(), Permission::Allow));
        assert!(matches!(Permission::parse("deny").unwrap(), Permission::Deny));
        assert!(matches!(Permission::parse("ask").unwrap(), Permission::Ask));
        assert!(matches!(Permission::parse("ask_once").unwrap(), Permission::AskOnce));
    }

    #[test]
    fn test_parse_blacklist() {
        let p = Permission::parse("blacklist: rm|shutdown|reboot").unwrap();
        if let Permission::Blacklist(patterns) = p {
            assert_eq!(patterns, vec!["rm", "shutdown", "reboot"]);
        } else {
            panic!("Expected Blacklist");
        }
    }

    #[test]
    fn test_parse_whitelist() {
        let p = Permission::parse("whitelist: ^curl|^wget").unwrap();
        if let Permission::Whitelist(patterns) = p {
            assert_eq!(patterns, vec!["^curl", "^wget"]);
        } else {
            panic!("Expected Whitelist");
        }
    }

    #[test]
    fn test_restrictiveness_order() {
        assert!(Permission::Deny.is_more_restrictive_than(&Permission::Allow));
        assert!(Permission::Ask.is_more_restrictive_than(&Permission::Blacklist(vec![])));
        assert!(Permission::Whitelist(vec![]).is_more_restrictive_than(&Permission::Ask));
        assert!(!Permission::Allow.is_more_restrictive_than(&Permission::Deny));
    }

    #[test]
    fn test_check_blacklist() {
        let perm = Permission::Blacklist(vec![r"rm\s+-rf".to_string(), "shutdown".to_string()]);
        assert!(matches!(check_permission(&perm, "ls -la"), PermissionDecision::Allow));
        assert!(matches!(check_permission(&perm, "rm -rf /tmp"), PermissionDecision::Deny(_)));
        assert!(matches!(check_permission(&perm, "shutdown now"), PermissionDecision::Deny(_)));
    }

    #[test]
    fn test_check_whitelist() {
        let perm = Permission::Whitelist(vec![r"^curl\s".to_string(), r"^wget\s".to_string()]);
        assert!(matches!(check_permission(&perm, "curl -s example.com"), PermissionDecision::Allow));
        assert!(matches!(check_permission(&perm, "wget example.com"), PermissionDecision::Allow));
        assert!(matches!(check_permission(&perm, "rm -rf /"), PermissionDecision::Deny(_)));
    }

    #[test]
    fn test_global_blocklist() {
        assert!(check_global_blocklist("rm -rf /", &[]).is_some());
        assert!(check_global_blocklist("mkfs.ext4 /dev/sda", &[]).is_some());
        assert!(check_global_blocklist("curl example.com", &[]).is_none());
        assert!(check_global_blocklist("ls -la", &[]).is_none());
    }

    #[test]
    fn test_global_blocklist_user_patterns() {
        let user = vec!["DROP\\s+TABLE".to_string()];
        assert!(check_global_blocklist("DROP TABLE users", &user).is_some());
        assert!(check_global_blocklist("SELECT * FROM users", &user).is_none());
    }

    #[test]
    fn test_merge_blacklist_extends() {
        let global = Permission::Blacklist(vec!["rm".to_string()]);
        let task = Permission::Blacklist(vec!["curl".to_string()]);
        let merged = global.merge_override(&task);
        if let Permission::Blacklist(patterns) = merged {
            assert!(patterns.contains(&"rm".to_string()));
            assert!(patterns.contains(&"curl".to_string()));
        } else {
            panic!("Expected Blacklist");
        }
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
