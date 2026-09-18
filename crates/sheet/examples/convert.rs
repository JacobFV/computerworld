//! Convert between the formats the engine reads and writes, over the host filesystem,
//! for checking files against other spreadsheet programs:
//! `cargo run -p cw-sheet --example convert -- in.xlsx out.ods`.
use cw_sheet::Workbook;

fn load(path: &str) -> Result<Workbook, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("xlsx") => cw_sheet::xlsx::read(&bytes),
        Some("ods") => cw_sheet::ods::read(&bytes),
        Some("csv") => Ok(cw_sheet::csv::read(
            &String::from_utf8_lossy(&bytes),
            "Sheet1",
        )),
        _ => Err(format!("unknown input type: {path}")),
    }
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: convert INPUT OUTPUT");
        std::process::exit(2);
    };
    let wb = match load(input) {
        Ok(wb) => wb,
        Err(e) => {
            eprintln!("{input}: {e}");
            std::process::exit(1);
        }
    };
    let bytes = match output
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("xlsx") => cw_sheet::xlsx::write(&wb, "Microsoft Excel"),
        Some("ods") => cw_sheet::ods::write(&wb, "LibreOffice"),
        Some("csv") => cw_sheet::csv::write(&wb, 0).into_bytes(),
        _ => {
            eprintln!("unknown output type: {output}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::write(output, bytes) {
        eprintln!("{output}: {e}");
        std::process::exit(1);
    }
    for (i, s) in wb.sheets.iter().enumerate() {
        println!(
            "sheet {i}: {} ({} cells, {} charts)",
            s.name,
            s.cells.len(),
            s.charts.len()
        );
    }
}
