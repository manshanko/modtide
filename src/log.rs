#[allow(dead_code)]
pub fn log(s: &str) {
    use std::io::Write;

    let mut fd = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("modtide-log.txt")
        .unwrap();
    writeln!(&mut fd, "{s}").unwrap();
}
