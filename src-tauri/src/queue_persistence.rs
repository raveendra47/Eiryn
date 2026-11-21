use crate::queue::QueueState;
use std::path::PathBuf;

pub fn save_queue(state: &QueueState, path: &PathBuf) {
    if let Ok(json) = serde_json::to_string(state) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

pub fn load_queue(path: &PathBuf) -> Option<QueueState> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}
