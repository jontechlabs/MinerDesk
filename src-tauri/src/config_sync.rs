//! Serial, coalescing background work: a save queues data, never waits for OS tasks.
//! This module uses only std so its real concurrency tests can run with rustc.
use std::{io, sync::mpsc::{self, Sender}, thread};

pub fn start_worker<T, F>(initial: T, mut apply: F) -> io::Result<Sender<T>>
where T: Send + 'static, F: FnMut(T) + Send + 'static {
    let (sender, receiver) = mpsc::channel();
    // Queue the initial snapshot before exposing the sender to other writers.
    sender.send(initial).map_err(|_| io::Error::other("config sync channel closed"))?;
    thread::Builder::new().name("minerdesk-config-sync".into()).spawn(move || {
        while let Ok(mut next) = receiver.recv() {
            // Intermediate saves may be superseded while Windows is still busy.
            for newer in receiver.try_iter() { next = newer; }
            apply(next);
        }
    })?;
    Ok(sender)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::{Arc, Mutex}, time::Duration};

    #[test]
    fn slow_platform_work_does_not_block_saves_and_latest_snapshot_wins() {
        let (started, has_started) = mpsc::channel();
        let (release, wait_for_release) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let applied = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&applied);
        let sender = start_worker(0, move |value| {
            record.lock().unwrap().push(value);
            if value == 0 { started.send(()).unwrap(); wait_for_release.recv().unwrap(); }
            else { done.send(value).unwrap(); }
        }).unwrap();
        has_started.recv_timeout(Duration::from_secs(2)).unwrap();
        // These sends must complete although the worker is still blocked above.
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        sender.send(3).unwrap();
        assert_eq!(*applied.lock().unwrap(), vec![0]);
        release.send(()).unwrap();
        assert_eq!(completed.recv_timeout(Duration::from_secs(2)).unwrap(), 3);
        assert_eq!(*applied.lock().unwrap(), vec![0, 3]);
    }

    #[test]
    fn later_saves_are_processed_after_the_first_apply() {
        let (done, completed) = mpsc::channel();
        let sender = start_worker("first", move |value| { done.send(value).unwrap(); }).unwrap();
        assert_eq!(completed.recv_timeout(Duration::from_secs(2)).unwrap(), "first");
        sender.send("next").unwrap();
        assert_eq!(completed.recv_timeout(Duration::from_secs(2)).unwrap(), "next");
    }
}
