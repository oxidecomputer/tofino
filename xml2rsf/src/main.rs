use anyhow::bail;
use anyhow::Result;

mod parse;
mod rsf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        bail!("usage: {} <xml defs>", args[0]);
    }

    let ir = parse::parse_xml(&args[1])?;
    let rsf = rsf::convert(ir)?;

    Ok(())
}
