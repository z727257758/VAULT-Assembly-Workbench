use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub type Cancel = Arc<AtomicBool>;
pub fn check(cancel: Option<&Cancel>) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err("预览任务已取消".into())
    } else {
        Ok(())
    }
}

/// One worker at a time; replace queued work and reject obsolete results.
pub struct Queue<T> {
    revision: u64,
    cancel: Cancel,
    pending: Option<(Instant, T)>,
}
impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            revision: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            pending: None,
        }
    }
}
impl<T> Queue<T> {
    pub fn schedule(&mut self, value: T, now: Instant, delay: Duration) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.revision += 1;
        self.pending = Some((now + delay, value));
    }
    pub fn take_ready(&mut self, now: Instant) -> Option<(u64, T)> {
        if self.pending.as_ref().is_some_and(|(due, _)| *due <= now) {
            self.pending.take().map(|(_, value)| (self.revision, value))
        } else {
            None
        }
    }
    pub fn cancellation(&self) -> Cancel {
        self.cancel.clone()
    }
    pub fn is_current(&self, revision: u64) -> bool {
        self.revision == revision
    }
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_changes_wait_for_last_edit() {
        let mut queue = Queue::default();
        let now = Instant::now();
        let delay = Duration::from_millis(300);
        queue.schedule("first", now, delay);
        queue.schedule("last", now + Duration::from_millis(250), delay);
        assert!(queue.take_ready(now + delay).is_none());
        let (revision, value) = queue.take_ready(now + Duration::from_millis(550)).unwrap();
        assert_eq!(value, "last");
        assert!(queue.is_current(revision));
        assert!(!queue.has_pending());
    }
    #[test]
    fn editing_during_load_invalidates_success_and_failure() {
        let mut queue = Queue::default();
        let now = Instant::now();
        queue.schedule("assembly", now, Duration::ZERO);
        let (old, _) = queue.take_ready(now).unwrap();
        let old_cancel = queue.cancellation();
        queue.schedule("new config", now, Duration::from_millis(300));
        assert!(!queue.is_current(old));
        assert!(check(Some(&old_cancel)).is_err());
        assert!(check(Some(&queue.cancellation())).is_ok());
        // Switching to a single part also supersedes pending assembly work.
        queue.schedule("single", now, Duration::ZERO);
        let (new, value) = queue.take_ready(now).unwrap();
        assert_eq!(value, "single");
        assert!(queue.is_current(new));
        assert!(!queue.is_current(old));
    }
}
