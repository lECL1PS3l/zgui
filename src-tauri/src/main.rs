#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Дочерний режим раннера теста: один elevated-процесс делает весь прогон.
    if args.get(1).map(String::as_str) == Some("--test-runner") {
        match args.get(2) {
            Some(plan) => {
                std::process::exit(zgui_lib::test_runner_main(std::path::Path::new(plan)))
            }
            None => std::process::exit(2),
        }
    }
    zgui_lib::run()
}
