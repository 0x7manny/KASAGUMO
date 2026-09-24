use kasagumo::FilePrimitive;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "./assets/kasagumo-logo.png".to_string());

    let file = match FilePrimitive::from_path(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("erreur sur {path} : {e}");
            std::process::exit(1);
        }
    };

    println!("{file}");
    for chunk in file.chunks() {
        println!("  {chunk}");
    }

    match file.verify() {
        Ok(()) => println!("intégrité : OK"),
        Err(e) => println!("intégrité : ÉCHEC ({e})"),
    }
}
