fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match abbey_learning::run(&args, &[]) {
        Ok(value) => println!("{value}"),
        Err(error) => {
            if let Some(help) = error.downcast_ref::<clap::Error>() {
                if matches!(
                    help.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) {
                    print!("{help}");
                    return;
                }
            }

            eprintln!(
                "{}",
                serde_json::json!({"error":format!("{error:#}"),"promoted":false})
            );
            std::process::exit(2);
        }
    }
}
