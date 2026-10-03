//! AI 診断の先読みキュー。
//!
//! 表示中の数枚先まで先に診断しておき、切り替えのたびに待たせない（外部 API は十数秒かかることがある）。
//! ワーカーは 1 つで順番に処理する（外部 API への同時リクエストを増やさない）。
//! 新しい予定を渡すと、まだ始めていない古い予定は捨てて置き換える（画像の切り替えに追従するため）。

use std::sync::{Arc, Condvar, Mutex};

/// 先読みの 1 件。実行する処理そのもの
pub type Job = Box<dyn FnOnce() + Send>;

struct Shared {
    queue: Mutex<Vec<Job>>,
    wake: Condvar,
}

pub struct DiagnosisPrefetcher {
    shared: Arc<Shared>,
}

impl DiagnosisPrefetcher {
    pub fn start() -> Self {
        let shared = Arc::new(Shared { queue: Mutex::new(Vec::new()), wake: Condvar::new() });
        let worker = Arc::downgrade(&shared);
        std::thread::spawn(move || loop {
            // 持ち主（DiagnosisPrefetcher）が捨てられたら終わる
            let Some(shared) = worker.upgrade() else { return };
            let job = {
                let mut q = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
                if q.is_empty() {
                    // 時々起きて、持ち主がいなくなっていないか確かめる
                    let (guard, _) = shared
                        .wake
                        .wait_timeout(q, std::time::Duration::from_millis(500))
                        .unwrap_or_else(|e| e.into_inner());
                    q = guard;
                }
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            };
            drop(shared);
            if let Some(job) = job {
                job();
            }
        });
        DiagnosisPrefetcher { shared }
    }

    /// 予定を置き換える（先頭が最も急ぐ）。実行中の 1 件は止めない
    pub fn schedule(&self, jobs: Vec<Job>) {
        *self.shared.queue.lock().unwrap_or_else(|e| e.into_inner()) = jobs;
        self.shared.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Sender};
    use std::time::Duration;

    fn job(tx: &Sender<&'static str>, name: &'static str) -> Job {
        let tx = tx.clone();
        Box::new(move || tx.send(name).unwrap())
    }

    #[test]
    fn jobs_run_in_order_on_one_worker() {
        let p = DiagnosisPrefetcher::start();
        let (tx, rx) = channel();
        p.schedule(vec![job(&tx, "a"), job(&tx, "b"), job(&tx, "c")]);
        let got: Vec<_> = (0..3).map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert_eq!(got, ["a", "b", "c"]);
    }

    #[test]
    fn a_new_schedule_replaces_jobs_that_have_not_started() {
        let p = DiagnosisPrefetcher::start();
        let (tx, rx) = channel();
        let (gate_tx, gate_rx) = channel::<()>();
        let (started_tx, started_rx) = channel::<()>();
        // 1 件目を実行中で止めておき、その間に予定を置き換える
        let tx1 = tx.clone();
        p.schedule(vec![
            Box::new(move || {
                started_tx.send(()).unwrap();
                gate_rx.recv().unwrap();
                tx1.send("running").unwrap();
            }),
            job(&tx, "old-1"),
            job(&tx, "old-2"),
        ]);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        p.schedule(vec![job(&tx, "new-1")]);
        gate_tx.send(()).unwrap();
        let got: Vec<_> = (0..2).map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert_eq!(got, ["running", "new-1"], "実行中の 1 件は完了し、古い予定は捨てられる");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "古い予定は実行されない");
    }

    #[test]
    fn the_worker_keeps_serving_after_the_queue_was_emptied() {
        let p = DiagnosisPrefetcher::start();
        let (tx, rx) = channel();
        p.schedule(Vec::new());
        p.schedule(vec![job(&tx, "x")]);
        p.schedule(Vec::new());
        // 空にしたあとでも、新しい予定は実行される
        p.schedule(vec![job(&tx, "y")]);
        let mut seen = Vec::new();
        while let Ok(n) = rx.recv_timeout(Duration::from_millis(500)) {
            seen.push(n);
        }
        assert!(seen.contains(&"y"));
    }
}
