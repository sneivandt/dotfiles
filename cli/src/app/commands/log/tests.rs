use super::*;

fn opts() -> LogOpts {
    LogOpts {
        run: None,
        id: None,
        task: None,
        raw: false,
        list: false,
        format: None,
        command: None,
        verbose: false,
    }
}

fn write_run(dir: &Path, name: &str, contents: &str) {
    std::fs::create_dir_all(dir).expect("create log dir");
    std::fs::write(dir.join(name), contents).expect("write log");
}

fn capture(dir: &Path, opts: &LogOpts, verbose: bool) -> String {
    let mut output = Vec::new();
    run_in_dir(dir, opts, verbose, &mut output).expect("log command should succeed");
    String::from_utf8(output).unwrap()
}

mod rendering;
mod selection;
