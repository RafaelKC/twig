use std::io::{self, BufRead, Write};

pub fn info(msg: impl AsRef<str>) {
    println!("\x1b[36m→ {}\x1b[0m", msg.as_ref());
}

pub fn ok(msg: impl AsRef<str>) {
    println!("\x1b[32m✔ {}\x1b[0m", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    eprintln!("\x1b[33m! {}\x1b[0m", msg.as_ref());
}

/// Prompt the user. Reads from the terminal when there is one; without a tty (pipe,
/// automated agent) falls back to stdin and, failing that, returns empty so the
/// caller picks the safe path.
pub fn ask(prompt: &str) -> String {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    #[cfg(unix)]
    if std::io::IsTerminal::is_terminal(&io::stdout()) {
        if let Ok(tty) = std::fs::File::open("/dev/tty") {
            let mut line = String::new();
            let _ = io::BufReader::new(tty).read_line(&mut line);
            return line.trim().to_string();
        }
    }

    let mut line = String::new();
    let _ = io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

pub fn confirm(prompt: &str) -> bool {
    matches!(ask(prompt).as_str(), "y" | "Y" | "s" | "S")
}
