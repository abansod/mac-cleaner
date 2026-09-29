use crossterm::event::KeyEvent;

use crate::engine::DeleteOutcome;
use crate::models::ScanResult;

pub enum Message {
    Key(KeyEvent),
    /// A list row was clicked.
    Select(usize),
    ScanProgress {
        message: String,
        index: usize,
        total: usize,
    },
    ScanFinished(ScanResult),
    /// The scan worker exited without sending a result.
    ScanStopped,
    DeleteProgress {
        message: String,
        done: u64,
        total: u64,
    },
    DeleteFinished(DeleteOutcome),
    /// The delete worker exited without sending an outcome.
    DeleteStopped,
}
