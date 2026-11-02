//! Одна операция управления VPN для окна, трея и автозапуска.
use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
pub struct Operation;
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}
pub fn acquire() -> Result<Operation, String> {
    ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| Operation)
        .map_err(|_| "Дождитесь завершения текущей операции VPN".into())
}
impl Drop for Operation {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prevents_overlap_and_releases_on_error() {
        let first = super::acquire().unwrap();
        assert!(super::acquire().is_err());
        drop(first);
        assert!(super::acquire().is_ok());
        assert!(!super::active());
    }
}
