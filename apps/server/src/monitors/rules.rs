//! Kuma 2.5.0's status rules as decision 0008 settles them: retries, upside down, quorum,
//! groups, important beats and notifications. Pure functions; the evaluator applies them.

use v1per_protocol::operations::monitors::Quorum;
use v1per_storage::Status;

/// What one check round found, before retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass(String),
    /// A failure that goes through retries: pending while retries are left, then down.
    Fail(String),
    /// A failure that is down at once (json-query with `retry_only_on_status_code_failure`).
    FailNow(String),
    /// A status set without a failure: a manual monitor's, or a group's pending. Retries reset.
    Set(Status, String),
}

impl Verdict {
    /// Upside-down mode for the monitors the server evaluates itself, as Kuma applies it after
    /// the check: a pass becomes a failure that retries, anything else becomes up. Pending
    /// stays pending. Probe results arrive already inverted from `v1per_checks`.
    pub fn upside_down(self) -> Self {
        match self {
            Verdict::Pass(_) => Verdict::Fail("Flip UP to DOWN".to_owned()),
            Verdict::Set(Status::Pending, message) => Verdict::Set(Status::Pending, message),
            Verdict::Fail(message)
            | Verdict::FailNow(message)
            | Verdict::Set(Status::Down | Status::Up | Status::Maintenance, message) => {
                Verdict::Pass(message)
            }
        }
    }
}

/// A status after retries, with the failures in a row it counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retried {
    pub status: Status,
    pub retries: u32,
    pub message: String,
}

/// Kuma's retry rule: a failure is pending while `retries < max_retries`, then down; the
/// count keeps rising while down and resets on anything else.
pub fn retry(verdict: Verdict, retries: u32, max_retries: u32) -> Retried {
    let (status, retries, message) = match verdict {
        Verdict::Pass(message) => (Status::Up, 0, message),
        Verdict::Set(status, message) => (status, 0, message),
        Verdict::FailNow(message) => (Status::Down, 0, message),
        Verdict::Fail(message) if max_retries > 0 && retries < max_retries => {
            (Status::Pending, retries + 1, message)
        }
        Verdict::Fail(message) => (Status::Down, retries.saturating_add(1), message),
    };
    Retried {
        status,
        retries,
        message,
    }
}

/// The monitor status over its live probes' statuses. Down by the quorum rule over down
/// probes; else pending by the same rule over down and pending ones; else up. No live probe
/// is pending.
pub fn quorum(rule: Quorum, probes: &[Status]) -> Status {
    let live = probes.len();
    if live == 0 {
        return Status::Pending;
    }
    let meets = |failing: usize| match rule {
        Quorum::Majority => failing * 2 >= live,
        Quorum::Any => failing > 0,
        Quorum::All => failing == live,
    };
    let down = probes.iter().filter(|s| **s == Status::Down).count();
    let pending = probes.iter().filter(|s| **s == Status::Pending).count();
    if down > 0 && meets(down) {
        Status::Down
    } else if down + pending > 0 && meets(down + pending) {
        Status::Pending
    } else {
        Status::Up
    }
}

/// One direct member of a group, by its latest monitor-level status.
pub struct Member {
    pub name: String,
    pub active: bool,
    pub status: Option<Status>,
}

/// Kuma's group check: empty is pending, paused members are skipped, a member without a
/// status is pending, maintenance counts as up. A down member is a failure, so the group's
/// own retries and upside down apply.
pub fn group(members: &[Member]) -> Verdict {
    if members.is_empty() {
        return Verdict::Set(Status::Pending, "Group empty".to_owned());
    }
    let mut down = Vec::new();
    let mut pending = Vec::new();
    for member in members.iter().filter(|m| m.active) {
        match member.status {
            Some(Status::Down) => down.push(member.name.as_str()),
            None | Some(Status::Pending) => pending.push(member.name.as_str()),
            Some(Status::Up | Status::Maintenance) => {}
        }
    }
    match (down.is_empty(), pending.is_empty()) {
        (true, true) => Verdict::Pass("All children up and running".to_owned()),
        (true, false) => Verdict::Set(
            Status::Pending,
            format!("Pending child monitors: {}", pending.join(", ")),
        ),
        (false, _) => {
            let mut message = format!("Child monitors down: {}", down.join(", "));
            if !pending.is_empty() {
                message.push_str(&format!("; pending: {}", pending.join(", ")));
            }
            Verdict::Fail(message)
        }
    }
}

/// How a monitor-level beat counts for alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub important: bool,
    pub notify: bool,
}

/// Kuma's `isImportantBeat` and `isImportantForNotification`, with its rule that a first beat
/// notifies only when it is down. `previous` is `None` on the first beat.
pub fn flags(previous: Option<Status>, current: Status) -> Flags {
    use Status::*;
    let Some(previous) = previous else {
        return Flags {
            important: true,
            notify: current == Down,
        };
    };
    let notify = matches!(
        (previous, current),
        (Maintenance, Down) | (Up, Down) | (Down, Up) | (Pending, Down)
    );
    let important = notify
        || matches!(
            (previous, current),
            (Down, Maintenance) | (Up, Maintenance) | (Maintenance, Up)
        );
    Flags { important, notify }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> Verdict {
        Verdict::Fail("boom".into())
    }

    /// Two retries: pending, pending, down, down; a pass resets.
    #[test]
    fn failures_retry_then_go_down_and_a_pass_resets() {
        let mut retries = 0;
        let mut seen = Vec::new();
        for verdict in [
            fail(),
            fail(),
            fail(),
            fail(),
            Verdict::Pass("ok".into()),
            fail(),
        ] {
            let retried = retry(verdict, retries, 2);
            retries = retried.retries;
            seen.push((retried.status, retried.retries));
        }
        use Status::*;
        assert_eq!(
            seen,
            [
                (Pending, 1),
                (Pending, 2),
                (Down, 3),
                (Down, 4),
                (Up, 0),
                (Pending, 1)
            ]
        );
        assert_eq!(retry(fail(), 0, 0).status, Down);
        assert_eq!(retry(Verdict::FailNow("x".into()), 0, 5).status, Down);
    }

    #[test]
    fn upside_down_turns_passes_into_retried_failures_and_leaves_pending() {
        assert_eq!(
            Verdict::Pass("ok".into()).upside_down(),
            Verdict::Fail("Flip UP to DOWN".into())
        );
        assert_eq!(fail().upside_down(), Verdict::Pass("boom".into()));
        let pending = Verdict::Set(Status::Pending, "Group empty".into());
        assert_eq!(pending.clone().upside_down(), pending);
    }

    #[test]
    fn quorum_rules_over_probe_statuses() {
        use Status::*;
        assert_eq!(quorum(Quorum::Majority, &[Down, Up]), Down);
        assert_eq!(quorum(Quorum::Majority, &[Down, Up, Up]), Up);
        assert_eq!(quorum(Quorum::Majority, &[Pending, Down, Up]), Pending);
        assert_eq!(quorum(Quorum::Any, &[Down, Up, Up]), Down);
        assert_eq!(quorum(Quorum::All, &[Down, Down, Up]), Up);
        assert_eq!(quorum(Quorum::All, &[Down, Down]), Down);
        assert_eq!(quorum(Quorum::Majority, &[]), Pending);
    }

    #[test]
    fn groups_follow_their_direct_members() {
        let member = |name: &str, active, status| Member {
            name: name.into(),
            active,
            status,
        };
        use Status::*;
        assert_eq!(group(&[]), Verdict::Set(Pending, "Group empty".into()));
        assert_eq!(
            group(&[
                member("a", true, Some(Up)),
                member("m", true, Some(Maintenance)),
                member("off", false, Some(Down)),
            ]),
            Verdict::Pass("All children up and running".into())
        );
        assert_eq!(
            group(&[member("a", true, Some(Up)), member("new", true, None)]),
            Verdict::Set(Pending, "Pending child monitors: new".into())
        );
        assert_eq!(
            group(&[
                member("a", true, Some(Down)),
                member("b", true, Some(Pending))
            ]),
            Verdict::Fail("Child monitors down: a; pending: b".into())
        );
    }

    /// Decision 0008's tables, row by row.
    #[test]
    fn important_and_notify_flags_match_kuma() {
        use Status::*;
        let cases = [
            (None, Up, true, false),
            (None, Down, true, true),
            (Some(Up), Pending, false, false),
            (Some(Up), Down, true, true),
            (Some(Up), Up, false, false),
            (Some(Pending), Down, true, true),
            (Some(Pending), Up, false, false),
            (Some(Down), Down, false, false),
            (Some(Down), Up, true, true),
            (Some(Maintenance), Maintenance, false, false),
            (Some(Maintenance), Up, true, false),
            (Some(Maintenance), Down, true, true),
            (Some(Down), Maintenance, true, false),
            (Some(Up), Maintenance, true, false),
        ];
        for (previous, current, important, notify) in cases {
            assert_eq!(
                flags(previous, current),
                Flags { important, notify },
                "{previous:?} -> {current:?}"
            );
        }
    }
}
