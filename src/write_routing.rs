//! Shared daemon-routing policy for MemPalace write operations.
//! Transport-agnostic policy matching upstream `write_routing.py`.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteRoutingPolicy {
    Direct,
    Prefer,
    Require,
}

impl WriteRoutingPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }
}

impl fmt::Display for WriteRoutingPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for WriteRoutingPolicy {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "direct" => Ok(Self::Direct),
            "prefer" => Ok(Self::Prefer),
            "require" => Ok(Self::Require),
            _ => bail!("invalid write routing policy '{}'; expected direct, prefer, or require", s),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteRoutingTarget {
    Direct,
    Daemon,
    Blocked,
}

impl WriteRoutingTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Daemon => "daemon",
            Self::Blocked => "blocked",
        }
    }
}

impl fmt::Display for WriteRoutingTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct RoutingPolicyCandidate {
    pub source: String,
    pub value: String,
    pub legacy_boolean: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedWriteRoutingPolicy {
    pub policy: WriteRoutingPolicy,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteRoutingDecision {
    pub policy: WriteRoutingPolicy,
    pub target: WriteRoutingTarget,
    pub auto_start_daemon: bool,
    pub reason: String,
}

impl WriteRoutingDecision {
    pub fn use_daemon(&self) -> bool {
        self.target == WriteRoutingTarget::Daemon
    }

    pub fn is_blocked(&self) -> bool {
        self.target == WriteRoutingTarget::Blocked
    }
}

pub fn parse_write_routing_policy(
    value: &str,
    legacy_boolean: bool,
) -> Result<WriteRoutingPolicy> {
    let normalized = value.trim().to_lowercase();
    if let Ok(policy) = WriteRoutingPolicy::from_str(&normalized) {
        return Ok(policy);
    }

    if legacy_boolean {
        match normalized.as_str() {
            "1" | "true" | "yes" | "on" | "daemon" => return Ok(WriteRoutingPolicy::Prefer),
            "0" | "false" | "no" | "off" => return Ok(WriteRoutingPolicy::Direct),
            _ => {}
        }
    }

    bail!(
        "invalid write routing policy '{}'; expected direct, prefer, or require",
        value
    );
}

pub fn resolve_write_routing_policy(
    candidates: &[RoutingPolicyCandidate],
    default_policy: WriteRoutingPolicy,
) -> Result<ResolvedWriteRoutingPolicy> {
    for candidate in candidates {
        if candidate.value.trim().is_empty() {
            continue;
        }
        match parse_write_routing_policy(&candidate.value, candidate.legacy_boolean) {
            Ok(policy) => {
                return Ok(ResolvedWriteRoutingPolicy {
                    policy,
                    source: candidate.source.clone(),
                });
            }
            Err(e) => {
                bail!("{}: {}", candidate.source, e);
            }
        }
    }

    Ok(ResolvedWriteRoutingPolicy {
        policy: default_policy,
        source: "default".to_string(),
    })
}

pub fn choose_write_route(
    policy: WriteRoutingPolicy,
    daemon_available: bool,
    daemon_can_start: bool,
) -> WriteRoutingDecision {
    if policy == WriteRoutingPolicy::Direct {
        return WriteRoutingDecision {
            policy,
            target: WriteRoutingTarget::Direct,
            auto_start_daemon: false,
            reason: "policy-direct".to_string(),
        };
    }

    if daemon_available {
        return WriteRoutingDecision {
            policy,
            target: WriteRoutingTarget::Daemon,
            auto_start_daemon: false,
            reason: "daemon-available".to_string(),
        };
    }

    if daemon_can_start {
        return WriteRoutingDecision {
            policy,
            target: WriteRoutingTarget::Daemon,
            auto_start_daemon: true,
            reason: "daemon-auto-start".to_string(),
        };
    }

    if policy == WriteRoutingPolicy::Prefer {
        return WriteRoutingDecision {
            policy,
            target: WriteRoutingTarget::Direct,
            auto_start_daemon: false,
            reason: "daemon-unavailable-fallback".to_string(),
        };
    }

    WriteRoutingDecision {
        policy,
        target: WriteRoutingTarget::Blocked,
        auto_start_daemon: false,
        reason: "daemon-required-unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_write_routing_policy() {
        assert_eq!(
            parse_write_routing_policy("direct", false).unwrap(),
            WriteRoutingPolicy::Direct
        );
        assert_eq!(
            parse_write_routing_policy("PREFER", false).unwrap(),
            WriteRoutingPolicy::Prefer
        );
        assert_eq!(
            parse_write_routing_policy("require", false).unwrap(),
            WriteRoutingPolicy::Require
        );

        // Legacy boolean tests
        assert_eq!(
            parse_write_routing_policy("true", true).unwrap(),
            WriteRoutingPolicy::Prefer
        );
        assert_eq!(
            parse_write_routing_policy("false", true).unwrap(),
            WriteRoutingPolicy::Direct
        );
        assert_eq!(
            parse_write_routing_policy("1", true).unwrap(),
            WriteRoutingPolicy::Prefer
        );
        assert_eq!(
            parse_write_routing_policy("0", true).unwrap(),
            WriteRoutingPolicy::Direct
        );

        assert!(parse_write_routing_policy("invalid", false).is_err());
    }

    #[test]
    fn test_choose_write_route() {
        // Direct policy
        let d = choose_write_route(WriteRoutingPolicy::Direct, false, false);
        assert_eq!(d.target, WriteRoutingTarget::Direct);
        assert!(!d.use_daemon());

        // Prefer policy with daemon available
        let d = choose_write_route(WriteRoutingPolicy::Prefer, true, false);
        assert_eq!(d.target, WriteRoutingTarget::Daemon);
        assert!(d.use_daemon());

        // Prefer policy with daemon unavailable but startable
        let d = choose_write_route(WriteRoutingPolicy::Prefer, false, true);
        assert_eq!(d.target, WriteRoutingTarget::Daemon);
        assert!(d.auto_start_daemon);

        // Prefer policy with daemon unavailable and unstartable -> fallback to direct
        let d = choose_write_route(WriteRoutingPolicy::Prefer, false, false);
        assert_eq!(d.target, WriteRoutingTarget::Direct);
        assert_eq!(d.reason, "daemon-unavailable-fallback");

        // Require policy with daemon unavailable and unstartable -> blocked (never direct!)
        let d = choose_write_route(WriteRoutingPolicy::Require, false, false);
        assert_eq!(d.target, WriteRoutingTarget::Blocked);
        assert!(d.is_blocked());
        assert_eq!(d.reason, "daemon-required-unavailable");
    }
}
