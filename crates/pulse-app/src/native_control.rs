//! Native exit classification: already-observed completion wins a stop tie.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action<T> {
    Exited(T),
    Terminate,
    Wait,
}
pub(crate) fn action<T>(observed: Option<T>, stopping: bool) -> Action<T> {
    match observed {
        Some(exit) => Action::Exited(exit),
        None if stopping => Action::Terminate,
        None => Action::Wait,
    }
}
/// True EOF may precede exit status publication. A single 100ms grace is
/// never renewed; failures and stops without read EOF cannot use it.
pub(crate) fn eof_grace(
    stopping: bool,
    failed: bool,
    eof: bool,
    now: std::time::Instant,
    deadline: &mut Option<std::time::Instant>,
) -> bool {
    if !stopping || failed || !eof {
        return false;
    }
    now < *deadline.get_or_insert(now + std::time::Duration::from_millis(100))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eof_stop_cannot_misclassify_an_observed_child_exit() {
        assert_eq!(action(Some(0), true), Action::Exited(0));
        assert_eq!(action(Some(7), true), Action::Exited(7));
        assert_eq!(action(Some(0), false), Action::Exited(0));
    }
    #[test]
    fn eof_grace_is_bounded_and_never_renewed() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let mut deadline = None;
        assert!(!eof_grace(true, false, false, now, &mut deadline));
        assert!(deadline.is_none());
        assert!(!eof_grace(true, true, true, now, &mut deadline));
        assert!(deadline.is_none());
        assert!(!eof_grace(false, false, true, now, &mut deadline));
        assert!(deadline.is_none());
        assert!(eof_grace(true, false, true, now, &mut deadline));
        assert!(eof_grace(
            true,
            false,
            true,
            now + Duration::from_millis(99),
            &mut deadline
        ));
        assert!(!eof_grace(
            true,
            false,
            true,
            now + Duration::from_millis(100),
            &mut deadline
        ));
        assert!(!eof_grace(
            true,
            false,
            true,
            now + Duration::from_secs(1),
            &mut deadline
        ));
        assert_eq!(deadline, Some(now + Duration::from_millis(100)));
    }

    #[test]
    fn stop_still_terminates_a_live_child() {
        assert_eq!(action::<u32>(None, true), Action::Terminate);
        assert_eq!(action::<u32>(None, false), Action::Wait);
    }
}
