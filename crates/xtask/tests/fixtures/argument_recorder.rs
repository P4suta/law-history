use std::io::Write;

fn main() {
    let output = std::env::var_os("LAW_HISTORY_TEST_ARGV_OUTPUT").unwrap();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let record = arguments.join("\0");
    if std::env::var_os("LAW_HISTORY_TEST_ARGV_APPEND").is_some() {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output)
            .unwrap();
        writeln!(file, "{record}").unwrap();
    } else {
        std::fs::write(output, record).unwrap();
    }
    if let Ok(image) = std::env::var("LAW_HISTORY_TEST_DOCKER_IMAGE") {
        let missing = std::env::var_os("LAW_HISTORY_TEST_CONTAINER_MISSING").is_some();
        let status = match arguments.first().map(String::as_str) {
            Some("build") => 0,
            Some("image") if arguments.get(1).is_some_and(|value| value == "inspect") => {
                println!("{image}");
                0
            }
            Some("inspect") if !missing => {
                println!("{}", std::env::var("LAW_HISTORY_TEST_OOM_VERDICT").unwrap_or("false".into()));
                std::env::var("LAW_HISTORY_TEST_INSPECTION_EXIT").map(|value| value.parse().unwrap()).unwrap_or(0)
            }
            Some("rm") if !missing => std::env::var("LAW_HISTORY_TEST_REMOVAL_EXIT").map(|value| value.parse().unwrap()).unwrap_or(0),
            Some("run") => {
                eprintln!("requested verification image is unavailable");
                23
            }
            _ => {
                eprintln!("owned verification container is absent");
                1
            }
        };
        std::process::exit(status);
    }
    let status = std::env::var("LAW_HISTORY_TEST_ARGV_EXIT")
        .map(|value| value.parse().unwrap())
        .unwrap_or(0);
    std::process::exit(status);
}
