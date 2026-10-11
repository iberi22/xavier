fn load(path: &str) -> Option<String> {
    if let Some(data) = read_file(path) {
        Some(data)
    } else {
        None
    }
}
