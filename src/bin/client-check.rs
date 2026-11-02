#[path = "../client_detect.rs"]
mod client_detect;
fn main() {
    let path = std::env::args_os().nth(1).expect("client root or element");
    match client_detect::detect(std::path::Path::new(&path)) {
        Ok(c) => println!(
            "{}: {}",
            c.element.display(),
            if c.known_142 {
                "Detected PW 1.4.2 Client"
            } else {
                "Unknown client executable"
            }
        ),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
