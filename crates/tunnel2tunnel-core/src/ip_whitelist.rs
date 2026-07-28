use ipnetwork::IpNetwork;
use std::net::IpAddr;

/// Evaluate an `ip_whitelist` rule set against a peer IP address.
///
/// Rules are newline-separated, processed top-to-bottom; first match wins.
/// - Plain IP or CIDR `192.168.1.0/24` → allow
/// - Glob pattern `10.0.?.?` → allow
/// - Regex `s/^10\\.0\\..*/` → allow
/// - `!` prefix on any of the above → deny
///
/// An empty whitelist (or whitelist with only blank lines) allows all addresses.
/// If rules exist but none match, the address is denied.
pub fn evaluate(rules: &str, peer_ip: &str) -> bool {
    let rules: Vec<&str> = rules
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if rules.is_empty() {
        return true;
    }

    for rule in &rules {
        let (deny, pattern) = if let Some(rest) = rule.strip_prefix('!') {
            (true, rest.trim())
        } else {
            (false, *rule)
        };

        if pattern_matches(pattern, peer_ip) {
            return !deny;
        }
    }

    false // non-empty whitelist with no match → deny
}

fn pattern_matches(pattern: &str, peer_ip: &str) -> bool {
    if let Some(regex_body) = try_extract_regex(pattern) {
        return regex::Regex::new(regex_body)
            .map(|r| r.is_match(peer_ip))
            .unwrap_or(false);
    }

    if pattern.contains('/') {
        return matches_cidr(pattern, peer_ip);
    }

    if pattern.contains('*') || pattern.contains('?') {
        return glob::Pattern::new(pattern)
            .map(|p| p.matches(peer_ip))
            .unwrap_or(false);
    }

    pattern == peer_ip
}

fn matches_cidr(pattern: &str, peer_ip: &str) -> bool {
    let Ok(network) = pattern.parse::<IpNetwork>() else {
        return false;
    };
    let Ok(ip) = peer_ip.parse::<IpAddr>() else {
        return false;
    };
    network.contains(ip)
}

/// Returns the regex body if `pattern` has the form `s/<regex>/[flags]`.
fn try_extract_regex(pattern: &str) -> Option<&str> {
    if !pattern.starts_with("s/") && !pattern.starts_with("s|") {
        return None;
    }
    let sep = &pattern[1..2];
    // body is between first and second separator
    pattern[2..].split(sep).next()
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn empty_whitelist_allows_all() {
        assert!(evaluate("", "1.2.3.4"));
        assert!(evaluate("   \n  ", "10.0.0.1"));
    }

    #[test]
    fn plain_ip_match() {
        assert!(evaluate("192.168.1.5", "192.168.1.5"));
        assert!(!evaluate("192.168.1.5", "192.168.1.6"));
    }

    #[test]
    fn cidr_match() {
        assert!(evaluate("192.168.1.0/24", "192.168.1.100"));
        assert!(!evaluate("192.168.1.0/24", "192.168.2.1"));
    }

    #[test]
    fn glob_match() {
        assert!(evaluate("10.0.?.?", "10.0.1.2"));
        assert!(!evaluate("10.0.?.?", "10.0.10.2"));
    }

    #[test]
    fn regex_match() {
        assert!(evaluate("s/^10\\.0\\..*/", "10.0.1.2"));
        assert!(!evaluate("s/^10\\.0\\..*/", "192.168.1.1"));
    }

    #[test]
    fn inversion_deny() {
        // deny one IP, allow CIDR → the deny wins for that IP
        let rules = "!192.168.1.5\n192.168.1.0/24";
        assert!(!evaluate(rules, "192.168.1.5"));
        assert!(evaluate(rules, "192.168.1.10"));
    }

    #[test]
    fn no_match_denies() {
        assert!(!evaluate("10.0.0.1", "192.168.1.1"));
    }
}
