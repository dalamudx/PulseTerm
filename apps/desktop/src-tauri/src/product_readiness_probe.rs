//! Validation-only delayed replies; never changes native admission or cancels native work.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Receipt {
    Document,
    Presentation,
}
impl Receipt {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "document" => Some(Self::Document),
            "presentation" => Some(Self::Presentation),
            _ => None,
        }
    }
}
struct Gate {
    receipt: Receipt,
    used: AtomicBool,
}
impl Gate {
    fn take(&self, receipt: Receipt) -> bool {
        self.receipt == receipt && !self.used.swap(true, Ordering::AcqRel)
    }
}
static FAULT: OnceLock<Option<Gate>> = OnceLock::new();
pub(super) async fn delay(receipt: Receipt) {
    let fault = FAULT.get_or_init(|| {
        Receipt::parse(&std::env::var("PULSE_WORKSPACE_READINESS_FAULT").ok()?).map(|receipt| {
            Gate {
                receipt,
                used: AtomicBool::new(false),
            }
        })
    });
    if fault.as_ref().is_some_and(|gate| gate.take(receipt)) {
        eprintln!("workspace.readiness_fault_started");
        tokio::time::sleep(std::time::Duration::from_millis(6500)).await;
        eprintln!("workspace.readiness_fault_finished");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_fixed_modes_are_accepted() {
        assert!(Receipt::parse("document") == Some(Receipt::Document));
        assert!(Receipt::parse("presentation") == Some(Receipt::Presentation));
        for value in ["", "Document", "document\n", "presentation ", "6500"] {
            assert!(Receipt::parse(value).is_none());
        }
    }
    #[test]
    fn wrong_receipt_does_not_consume_single_use_fault() {
        let gate = Gate {
            receipt: Receipt::Presentation,
            used: AtomicBool::new(false),
        };
        assert!(!gate.take(Receipt::Document));
        assert!(gate.take(Receipt::Presentation));
        assert!(!gate.take(Receipt::Presentation));
        assert!(!gate.take(Receipt::Document));
    }
}
