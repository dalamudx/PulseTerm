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
    fn stop_still_terminates_a_live_child() {
        assert_eq!(action::<u32>(None, true), Action::Terminate);
        assert_eq!(action::<u32>(None, false), Action::Wait);
    }
}
