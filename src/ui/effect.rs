use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crate::engine::{self, ScanKind};
use crate::models::FileItem;

use super::message::Message;

/// Side effects requested by `update`. The runtime performs them.
#[derive(Debug)]
pub enum Command {
    None,
    Scan(ScanKind),
    Delete { items: Vec<FileItem>, bytes: u64 },
    Quit,
}

/// A background worker whose progress arrives as messages.
pub struct Task {
    rx: Receiver<Message>,
    stopped: fn() -> Message,
}

impl Task {
    /// Moves pending messages into `out`. Returns false once the worker is done.
    pub fn drain(&self, out: &mut Vec<Message>) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(message) => {
                    let finished = matches!(
                        message,
                        Message::ScanFinished(_) | Message::DeleteFinished(_)
                    );
                    out.push(message);
                    if finished {
                        return false;
                    }
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    out.push((self.stopped)());
                    return false;
                }
            }
        }
    }
}

pub fn scan(kind: ScanKind) -> Task {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = engine::scan(kind, &mut |message, index, total| {
            let _ = tx.send(Message::ScanProgress {
                message: message.to_string(),
                index,
                total,
            });
        });
        let _ = tx.send(Message::ScanFinished(result));
    });
    Task {
        rx,
        stopped: || Message::ScanStopped,
    }
}

pub fn delete(items: Vec<FileItem>, bytes: u64) -> Task {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome =
            engine::delete_items_with_progress(&items, bytes, &mut |message, done, total| {
                let _ = tx.send(Message::DeleteProgress {
                    message: message.to_string(),
                    done,
                    total,
                });
            });
        let _ = tx.send(Message::DeleteFinished(outcome));
    });
    Task {
        rx,
        stopped: || Message::DeleteStopped,
    }
}
