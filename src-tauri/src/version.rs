//! Version parsing and comparison for the in-app updater.
//! Dependency-free so it can be tested with plain `rustc --test`.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ver {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// `Some(n)` for test builds (e.g. `v0.1.1-test.3`), `None` for releases.
    pub test: Option<u64>,
}

pub fn parse(s: &str) -> Option<Ver> {
    let s = s.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'));
    let (core, rest) = match split {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    let core = core.trim_end_matches('.');
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    let test = if rest.is_empty() {
        None
    } else {
        let digits: Vec<char> = rest.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
        let digits: String = digits.into_iter().rev().collect();
        Some(digits.parse().unwrap_or(0))
    };
    Some(Ver { major, minor, patch, test })
}

impl Ord for Ver {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (self.test, o.test) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(&b),
            })
    }
}

impl PartialOrd for Ver {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// True when `latest` is newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        // Unknown current version: offer the update.
        (Some(_), None) => true,
        _ => false,
    }
}

/// Human readable label: "v0.1.1 test 3" instead of "0.1.1-test.3".
pub fn readable(s: &str) -> String {
    match parse(s) {
        Some(v) => match v.test {
            Some(t) => format!("v{}.{}.{} test {}", v.major, v.minor, v.patch, t),
            None => format!("v{}.{}.{}", v.major, v.minor, v.patch),
        },
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_same_newer() {
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("v0.10.0", "0.9.0"));
    }

    #[test]
    fn test_builds() {
        // test build on top of a release
        assert!(is_newer("v0.1.1-test.2", "0.1.0"));
        // release replaces its test build
        assert!(is_newer("v0.1.1", "0.1.1-test.2"));
        // test build never downgrades a release of the same version
        assert!(!is_newer("v0.1.1-test.3", "0.1.1"));
        // newer test build
        assert!(is_newer("v0.1.1-test.3", "0.1.1-test.2"));
    }

    #[test]
    fn labels() {
        assert_eq!(readable("0.1.1-test.3"), "v0.1.1 test 3");
        assert_eq!(readable("v0.2.0"), "v0.2.0");
    }

    #[test]
    fn garbage() {
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(is_newer("v0.1.0", "unknown"));
    }
}
