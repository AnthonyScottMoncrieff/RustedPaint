use rustedpaint::Document;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage:\n  rustedpaint inspect INPUT.(png|pdn)\n  rustedpaint render INPUT.(png|pdn) OUTPUT.png";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("RustedPaint: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        #[cfg(feature = "desktop")]
        [command] if command == "edit" => rustedpaint::desktop::run()?,
        [command] if command == "--help" || command == "-h" => println!("{USAGE}"),
        [command, input] if command == "inspect" => inspect(Path::new(input))?,
        [command, input, output] if command == "render" => {
            render(Path::new(input), Path::new(output))?
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn open(path: &Path) -> Result<Document, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut input = BufReader::new(file);
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("pdn") => {
            Document::read_pdn(&mut input).map_err(Into::into)
        }
        Some(extension) if extension.eq_ignore_ascii_case("png") => {
            Document::read_png(input).map_err(Into::into)
        }
        _ => Err(format!("unsupported input extension: {}", path.display()).into()),
    }
}

fn inspect(path: &Path) -> Result<(), Box<dyn Error>> {
    let document = open(path)?;
    println!(
        "{} × {} pixels, {} layers (bottom to top)",
        document.width(),
        document.height(),
        document.layers().len()
    );
    for (index, layer) in document.layers().iter().enumerate() {
        let visibility = if layer.is_visible() {
            "visible"
        } else {
            "hidden"
        };
        println!(
            "{}. {:?} — {visibility}, opacity {}/255, {:?}",
            index + 1,
            layer.name(),
            layer.opacity(),
            layer.blend_mode()
        );
    }
    Ok(())
}

fn render(input: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err(format!("refusing to overwrite {}", output.display()).into());
    }
    let document = open(input)?;
    let file = File::create(output)?;
    document.write_png(BufWriter::new(file))?;
    Ok(())
}
