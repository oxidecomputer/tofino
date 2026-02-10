use ::rsf::ast::Emit;
use anyhow::Result;
use anyhow::bail;

mod parse;
mod rsf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        bail!("usage: {} <xml defs> <rsf target>", args[0]);
    }

    let ir = parse::parse_xml(&args[1])?;
    let rsf = rsf::convert(ir)?;
    std::fs::write(&args[2], rsf.to_code().trim()).map_err(|e| e.into())
}
