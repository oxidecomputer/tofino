use std::collections::BTreeMap;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use convert_case::{Case, Casing};

use crate::parse::Node;
use crate::parse::RegMap;
use crate::parse::Register;

#[derive(Debug)]
struct Block<'a> {
    pub full_name: String,
    pub name: String,
    pub offset: u32,
    pub blocks: BTreeMap<String, Block<'a>>,
    pub regs: BTreeMap<String, &'a Register>,
}

impl<'a> Block<'a> {
    pub fn new(full_name: String, name: String, offset: u32) -> Self {
        Block {
            full_name,
            name,
            offset,
            blocks: BTreeMap::new(),
            regs: BTreeMap::new(),
        }
    }
}

type RegIndex<'a> = BTreeMap<String, &'a Register>;

fn update_block_tree<'a>(root: &mut Block<'a>, reg: &'a Register) {
    let mut path: Vec<&str> = reg
        .ref_name
        .strip_prefix("jbay_reg.")
        .unwrap()
        .split('.')
        .collect();
    let mut b = root;
    let reg_name = path.pop().unwrap().to_string();
    let mut full_name = "tofino_regs".to_string();
    for p in path {
        full_name = format!("{full_name}.{p}");
        b = b.blocks.entry(p.to_string()).or_insert(Block::new(
            full_name.clone(),
            p.to_string(),
            reg.offset,
        ));
        if b.offset < reg.offset {
            b.offset = reg.offset;
        }
    }
    b.regs.insert(reg_name, reg);
}

fn build_block_tree<'a>(map: &'a RegMap, idx: &RegIndex) -> Result<Block<'a>> {
    let mut root =
        Block::new("tofino_regs".to_string(), "tofino_regs".to_string(), 0);
    for r in &map.registers {
        update_block_tree(&mut root, r);
    }
    Ok(root)
}

fn all_block_names(block: &Block, names: &mut Vec<String>) {
    names.push(block.full_name.clone());
    for b in block.blocks.values() {
        all_block_names(b, names);
    }
}

// In RSF, all elements live in a single global namespace.  To avoid having to
// refer to each element by its full path name (as is done in the tofino map),
// we want to find the shortest name we can use for each element than still
// allows it to be uniquely identified.
fn uniquify(names: Vec<String>) -> BTreeMap<String, String> {
    struct Scratch {
        name: String,
        remaining: Vec<String>,
    }
    type ScratchMap = BTreeMap<String, Vec<Scratch>>;

    let mut map = ScratchMap::new();
    for name in names.into_iter() {
        let mut remaining = name
            .split('.')
            .map(|s| s.to_string().to_case(Case::UpperCamel))
            .collect::<Vec<String>>();
        let key = remaining.pop().unwrap();
        map.entry(key)
            .or_insert(Vec::new())
            .push(Scratch { name, remaining });
    }
    let mut uniq = BTreeMap::<String, String>::new();
    while !map.is_empty() {
        let mut good = map
            .iter()
            .filter_map(
                |(key, entry)| {
                    if entry.len() == 1 {
                        Some(key)
                    } else {
                        None
                    }
                },
            )
            .cloned()
            .collect::<Vec<String>>();
        while let Some(key) = good.pop() {
            uniq.insert(map.remove(&key).unwrap().pop().unwrap().name, key);
        }

        let mut new_map = ScratchMap::new();
        for (key, mut entries) in map {
            let all = entries
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<String>>();
            for mut entry in entries {
                if entry.remaining.is_empty() {
                    eprintln!(
                        "About to die on {} in {}.  Conflicts: {:?}",
                        key, entry.name, all
                    );
                }
                let new_key =
                    format!("{}_{}", entry.remaining.pop().unwrap(), key);
                new_map.entry(new_key).or_insert(Vec::new()).push(entry)
            }
        }
        map = new_map
    }
    uniq
}

fn build_reg_index<'a>(root: &'a RegMap) -> Result<RegIndex> {
    let mut idx = BTreeMap::new();
    for r in &root.registers {
        if idx.insert(r.ref_name.clone(), r).is_some() {
            bail!("multiple definitions of {}", r.ref_name);
        }
    }
    Ok(idx)
}

pub fn convert(map: RegMap) -> Result<String> {
    let reg_index = build_reg_index(&map)?;
    let reg_name_map =
        uniquify(map.registers.iter().map(|r| r.ref_name.clone()).collect());

    let block_tree = build_block_tree(&map, &reg_index)?;
    let mut block_names = Vec::<String>::new();
    all_block_names(&block_tree, &mut block_names);
    let block_name_map = uniquify(block_names);

    Ok("foo".to_string())
}
