use std::time::{SystemTime, UNIX_EPOCH};

use crate::process::run_capture;

pub(crate) fn timestamp() -> String {
    run_capture("date", &["+%Y-%m-%dT%H:%M:%S%z"]).unwrap_or_else(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string())
    })
}

pub(crate) fn capture_stamp() -> String {
    run_capture("date", &["+%Y%m%d-%H%M%S"]).unwrap_or_else(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string())
    })
}
