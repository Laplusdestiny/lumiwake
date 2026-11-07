//! 重い処理（デコード）を同時にいくつまで走らせるかの制限。

use std::sync::{Condvar, Mutex};

/// 同時に `max` 個までしか実行しない。空きがなければ空くまで待つ
pub struct DecodeLimit {
    running: Mutex<usize>,
    freed: Condvar,
    max: usize,
}

impl DecodeLimit {
    pub fn new(max: usize) -> Self {
        DecodeLimit { running: Mutex::new(0), freed: Condvar::new(), max: max.max(1) }
    }

    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        while *running >= self.max {
            running = self.freed.wait(running).unwrap_or_else(|e| e.into_inner());
        }
        *running += 1;
        drop(running);
        // f が panic しても枠を戻す
        let _slot = Slot(self);
        f()
    }
}

struct Slot<'a>(&'a DecodeLimit);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self.0.running.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        self.0.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn never_runs_more_than_max_at_once() {
        let limit = Arc::new(DecodeLimit::new(3));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..12)
            .map(|_| {
                let (limit, running, peak) = (limit.clone(), running.clone(), peak.clone());
                std::thread::spawn(move || {
                    limit.run(|| {
                        let now = running.fetch_add(1, SeqCst) + 1;
                        peak.fetch_max(now, SeqCst);
                        std::thread::sleep(Duration::from_millis(20));
                        running.fetch_sub(1, SeqCst);
                    })
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(peak.load(SeqCst), 3);
    }

    #[test]
    fn returns_the_closure_result() {
        assert_eq!(DecodeLimit::new(1).run(|| 42), 42);
    }

    #[test]
    fn frees_the_slot_even_if_the_closure_panics() {
        let limit = Arc::new(DecodeLimit::new(1));
        let l = limit.clone();
        assert!(std::thread::spawn(move || l.run(|| panic!("デコード失敗"))).join().is_err());
        assert_eq!(limit.run(|| 1), 1, "枠が戻っていなければここで止まる");
    }
}
