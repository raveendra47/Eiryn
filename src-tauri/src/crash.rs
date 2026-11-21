use std::backtrace::Backtrace;
use std::fs;
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn setup_panic_hook(data_dir: PathBuf) {
    let _ = fs::create_dir_all(&data_dir);

    let prev = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        prev(info);

        let crash_dir = data_dir.join("crashes");
        let _ = fs::create_dir_all(&crash_dir);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let path = crash_dir.join(format!("crash_{}.log", timestamp));

        let mut file = match fs::File::create(&path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let _ = writeln!(file, "Eiryn Crash Report");
        let _ = writeln!(file, "=================");

        if let Some(s) = info.payload().downcast_ref::<&str>() {
            let _ = writeln!(file, "Message: {}", s);
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            let _ = writeln!(file, "Message: {}", s);
        }

        if let Some(location) = info.location() {
            let _ = writeln!(file, "Location: {}:{}", location.file(), location.line());
        }

        let _ = writeln!(
            file,
            "Thread: {:?}",
            std::thread::current().name().unwrap_or("<unnamed>")
        );

        let backtrace = Backtrace::force_capture();
        let _ = writeln!(file, "\nBacktrace:\n{}", backtrace);

        let _ = fs::write(data_dir.join(".crashed"), timestamp.to_string());
    }));
}

pub fn clean_up_crash_marker(data_dir: &std::path::Path) {
    let marker = data_dir.join(".crashed");
    if marker.exists() {
        let _ = fs::remove_file(&marker);
    }
}
