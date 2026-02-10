use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::convert::From;
use std::sync::OnceLock;

use anyhow::Result;
use convert_case::{Case, Casing};
use regex::Regex;
use rsf::ast;
use rsf::common::FieldMode;
use rsf::common::FieldType;
use rsf::common::Number;
use rsf::common::NumberFormat;
use rsf::common::Span;

use crate::parse::AccessMode;
use crate::parse::Bitfield;
use crate::parse::RegMap;
use crate::parse::Register;

struct Regexes {
    pub normalize_re: regex::Regex,
    pub single_re: regex::Regex,
    pub range_re: regex::Regex,
}

static REGEXES: OnceLock<Regexes> = OnceLock::new();

fn init_regexes() {
    std::thread::spawn(|| {
        let _ = REGEXES.get_or_init(|| Regexes {
            normalize_re: regex::Regex::new(r"\[[\d -]+\]").unwrap(),
            single_re: Regex::new(r"\.*\[\s*(\d+)\s*\]").unwrap(),
            range_re: Regex::new(r"\.*\[(\d+)\s*-\s*(\d+)\]").unwrap(),
        });
    })
    .join()
    .unwrap()
}

struct Indexes {
    map: RegMap,
    registers: BTreeMap<String, String>,
    blocks: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct Block<'a> {
    full_name: String,
    name: String,
    low_offset: u32,
    high_offset: u32,
    count: u32,   // number of copies of the block
    spacing: u32, // bytes between the copies of the block
    blocks: BTreeMap<String, Block<'a>>,
    regs: BTreeMap<String, RegisterInstance<'a>>,
}

#[derive(Debug, Clone)]
struct RegisterInstance<'a> {
    instance_name: String,
    reference: &'a Register,
    low_offset: u32,
    high_offset: u32,
    count: u32,   // number of copies of the block
    spacing: u32, // bytes between the copies of the block
}

impl<'a> Block<'a> {
    pub fn new(
        full_name: String,
        name: String,
        low_offset: u32,
        high_offset: u32,
    ) -> Self {
        Block {
            full_name,
            name,
            low_offset,
            high_offset,
            count: 1,
            spacing: 0,
            blocks: BTreeMap::new(),
            regs: BTreeMap::new(),
        }
    }
}

fn build_block_tree<'a>(map: &'a RegMap) -> Result<Block<'a>> {
    let reg_index = map
        .registers
        .iter()
        .map(|r| (r.ref_name.clone(), r))
        .collect::<BTreeMap<String, &Register>>();

    let mut root = Block::new("main".to_string(), "main".to_string(), 0, 0);
    for a in &map.address_maps {
        for e in &a.entries {
            let Some(name) = &e.name else {
                continue;
            };

            let reg = match &e.ref_name {
                Some(ref_name) => {
                    reg_index.get(ref_name).map(|r| RegisterInstance {
                        instance_name: name.clone(),
                        reference: r,
                        low_offset: e.low,
                        high_offset: e.high,
                        count: 1,
                        spacing: 0,
                    })
                }
                None => None,
            };
            let mut path: Vec<&str> = match name.strip_prefix("jbay_reg.") {
                Some(x) => x.split('.').rev().collect(),
                None => continue,
            };
            let mut b = &mut root;
            let mut full_name = "main".to_string();
            while let Some(p) = path.pop() {
                if path.is_empty()
                    && let Some(reg) = reg
                {
                    b.regs.insert(p.to_string(), reg);
                    break;
                } else {
                    full_name = format!("{full_name}.{p}");
                    b = b.blocks.entry(p.to_string()).or_insert(Block::new(
                        full_name.clone(),
                        p.to_string(),
                        e.low,
                        e.high,
                    ));
                }
            }
        }
    }
    Ok(root)
}

#[derive(Debug)]
struct Array {
    pub low_idx: u32,
    pub high_idx: u32,
    pub low_offset: u32,
    pub high_offset: u32,
    pub elements: Vec<String>,
}

struct ArrayBuilderState {
    pub arrays: BTreeMap<String, Array>,
}

impl ArrayBuilderState {
    pub fn new() -> Self {
        ArrayBuilderState { arrays: BTreeMap::new() }
    }

    pub fn reset(&mut self) {
        self.arrays = BTreeMap::new();
    }

    pub fn check_object(
        &mut self,
        full_name: &str,
        low_offset: u32,
        high_offset: u32,
    ) {
        let Some(bracket) = full_name.find('[') else {
            return;
        };
        let name = String::from(&full_name[0..bracket]);
        let (low_idx, high_idx) = if let Some(c) =
            REGEXES.get().unwrap().single_re.captures(full_name)
        {
            let idx = c.get(1).unwrap().as_str().parse::<u32>().unwrap();
            (idx, idx)
        } else if let Some(c) =
            REGEXES.get().unwrap().range_re.captures(full_name)
        {
            let low = c.get(1).unwrap().as_str().parse::<u32>().unwrap();
            let high = c.get(2).unwrap().as_str().parse::<u32>().unwrap();
            (low, high)
        } else {
            return;
        };

        let array = self.arrays.entry(name).or_insert(Array {
            low_idx,
            high_idx,
            low_offset,
            high_offset,
            elements: Vec::new(),
        });
        array.low_offset = std::cmp::min(array.low_offset, low_offset);
        array.high_offset = std::cmp::max(array.high_idx, high_offset);
        array.low_idx = std::cmp::min(array.low_idx, low_idx);
        array.high_idx = std::cmp::max(array.high_idx, high_idx);
        array.elements.push(full_name.to_string());
    }
}

// Remove any bracketed numbers/ranges from a string, to convert a name with
// array indices or definitions into a clean nam.
fn name_normalize(name: &str) -> String {
    REGEXES.get().unwrap().normalize_re.replace_all(name, "").to_string()
}

// Walk the block tree looking for repeated objects that should be represented
// as arrays.  The detection is simply based on the object name, where a
// subscript notation indicates an array.  The tofino xml includes two different
// varieties of array descriptions.  One in which a single object explicitly
// indicates that it contains an array, and one in which each element of the
// array is identified separately.  Both can be seen in the lftlr.hash
// registers:
//     jbay_reg.device_select.lfltr[2].hash[1].hash_array[0 - 15]
//     jbay_reg.device_select.lfltr[2].hash[2].hash_array[0 - 15]
//     jbay_reg.device_select.lfltr[2].hash[3].hash_array[0 - 15]
fn build_arrays(state: &mut ArrayBuilderState, tree: &mut Block) {
    for b in tree.blocks.values_mut() {
        build_arrays(state, b);
    }
    state.reset();
    for b in tree.blocks.values_mut() {
        state.check_object(&b.name, b.low_offset, b.high_offset);
    }
    for (name, array) in &mut state.arrays {
        let mut block = tree.blocks.get(&array.elements[0]).unwrap().clone();
        block.name = name.clone();
        block.count = (array.high_idx + 1) - array.low_idx;
        block.spacing =
            ((array.high_offset + 1) - array.low_offset) / block.count;
        block.low_offset = array.low_offset;
        block.high_offset = array.high_offset;
        while let Some(e) = array.elements.pop() {
            tree.blocks.remove(&e);
        }
        tree.blocks.insert(block.name.clone(), block);
    }

    state.reset();
    for r in tree.regs.values_mut() {
        let instance_name = r.instance_name.rsplit(".").next().unwrap();
        state.check_object(instance_name, r.low_offset, r.high_offset);
    }
    for (name, array) in &mut state.arrays {
        let mut reg = tree.regs.get_mut(&array.elements[0]).unwrap().clone();
        reg.instance_name = name.clone();
        reg.count = (array.high_idx + 1) - array.low_idx;
        reg.spacing = ((array.high_offset + 1) - array.low_offset) / reg.count;
        reg.low_offset = array.low_offset;
        reg.high_offset = array.high_offset;
        while let Some(e) = array.elements.pop() {
            tree.regs.remove(&e);
        }
        tree.regs.insert(name.clone(), reg);
    }
}

fn all_block_names(block: &Block, names: &mut BTreeSet<String>) {
    names.insert(name_normalize(&block.full_name));
    for b in block.blocks.values() {
        all_block_names(b, names);
    }
}

// In RSF, all elements live in a single global namespace.  To avoid having to
// refer to each element by its full path name (as is done in the tofino map),
// we want to find the shortest name we can use for each element than still
// allows it to be uniquely identified.
fn uniquify(names: Vec<String>) -> BTreeMap<String, String> {
    #[derive(Debug)]
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
        map.entry(key).or_default().push(Scratch { name, remaining });
    }
    let mut uniq = BTreeMap::<String, String>::new();
    while !map.is_empty() {
        let mut good = map
            .iter()
            .filter_map(
                |(key, entry)| {
                    if entry.len() == 1 { Some(key) } else { None }
                },
            )
            .cloned()
            .collect::<Vec<String>>();
        while let Some(key) = good.pop() {
            uniq.insert(map.remove(&key).unwrap().pop().unwrap().name, key);
        }

        let mut new_map = ScratchMap::new();
        for (key, entries) in map {
            let all =
                entries.iter().map(|e| e.name.clone()).collect::<Vec<String>>();
            for mut entry in entries {
                if entry.remaining.is_empty() {
                    eprintln!(
                        "About to die on {} in {}.  Conflicts: {:?}",
                        key, entry.name, all
                    );
                }
                let new_key =
                    format!("{}_{}", entry.remaining.pop().unwrap(), key);
                new_map.entry(new_key).or_default().push(entry)
            }
        }
        map = new_map
    }
    uniq
}

fn ast_decimal(x: impl Into<u128>) -> Number {
    Number::new(x.into(), NumberFormat::decimal())
}

fn ast_hex(x: impl Into<u128>) -> ast::Number {
    Number::new(x.into(), NumberFormat::hex())
}

fn register_to_rsf(idx: &Indexes, register: &Register) -> ast::Register {
    // Remove the padding representing unused bitfields, and sort the
    // remaining bitfields into ascending order.
    let mut bitfields: Vec<Bitfield> = register
        .bitfields
        .iter()
        .filter(|b| b.id.as_str() != "_")
        .cloned()
        .collect();
    for b in &mut bitfields {
        b.id = b.id.replace(['[', ']'], "");
    }
    bitfields.sort_by_key(|b| b.lsb);

    let fields = bitfields
        .iter()
        .map(|f| {
            let id = ast::Identifier::new(&f.id);
            ast::Field {
                id: id.clone(),
                doc: vec![f.id.to_string()],
                mode: match f.access {
                    AccessMode::ReadOnly => FieldMode::ReadOnly,
                    AccessMode::ReadWrite => FieldMode::ReadWrite,
                    AccessMode::ReadWrite1Clear => FieldMode::ReadWrite,
                    AccessMode::WriteOnly => FieldMode::WriteOnly,
                },
                typ: FieldType::Bitfield { width: ast_hex(f.msb - f.lsb + 1) },
                offset: ast_hex(f.lsb),
                attrs: Vec::new(),
            }
        })
        .collect();

    let id =
        ast::Identifier::new(idx.registers.get(&register.ref_name).unwrap());
    ast::Register {
        id,
        doc: vec![register.title.to_string()],
        width: ast_decimal(32u32),
        reset_value: register.reset_value.map(ast_hex),
        sram: false,
        attrs: Vec::new(),
        fields,
    }
}

fn register_to_element(
    idx: &Indexes,
    base_offset: u32,
    register: &RegisterInstance,
) -> ast::BlockElement {
    let reference = register.reference;
    let path =
        ast::Identifier::new(idx.registers.get(&reference.ref_name).unwrap());
    let id = ast::Identifier::new(match reference.id.as_str() {
        "match" => "matchh",
        x => x,
    });
    ast::BlockElement {
        doc: vec![reference.ref_name.to_string()],
        component: if register.count == 1 {
            ast::Component::Single {
                id,
                typ: ast::QualifiedType { path: vec![path], span: Span::Any },
            }
        } else {
            ast::Component::Array {
                id,
                typ: ast::QualifiedType { path: vec![path], span: Span::Any },
                length: ast_hex(register.count),
                spacing: ast_hex(register.spacing),
            }
        },
        offset: ast_hex(register.low_offset - base_offset),
        attrs: Vec::new(),
    }
}

fn block_to_element(
    idx: &Indexes,
    base_offset: u32,
    block: &Block,
) -> ast::BlockElement {
    let clean = idx.blocks.get(&name_normalize(&block.full_name)).unwrap();
    let path = ast::Identifier::new(clean);
    let id_name = block.name.clone();
    let id = ast::Identifier::new(match id_name.as_str() {
        "match" => "matchh",
        x => x,
    });

    ast::BlockElement {
        doc: vec![block.name.to_string()],
        component: if block.count == 1 {
            ast::Component::Single {
                id,
                typ: ast::QualifiedType { path: vec![path], span: Span::Any },
            }
        } else {
            ast::Component::Array {
                id,
                typ: ast::QualifiedType { path: vec![path], span: Span::Any },
                length: ast_hex(block.count),
                spacing: ast_hex(block.spacing),
            }
        },
        offset: ast_hex(block.low_offset - base_offset),
        attrs: Vec::new(),
    }
}

fn block_to_rsf(idx: &Indexes, block: &Block) -> ast::Block {
    let mut elements = Vec::new();
    for b in block.blocks.values() {
        elements.push(block_to_element(idx, block.low_offset, b));
    }
    for r in block.regs.values() {
        elements.push(register_to_element(idx, block.low_offset, r));
    }
    elements.sort_by_key(|e| e.offset.value);
    let clean = idx.blocks.get(&name_normalize(&block.full_name)).unwrap();
    ast::Block {
        id: ast::Identifier::new(clean),
        doc: vec![block.full_name.to_string()],
        sram: false,
        attrs: Vec::new(),
        elements,
    }
}

fn all_blocks_to_rsf(idx: &Indexes, block: &Block) -> Vec<ast::Block> {
    let mut ast = vec![block_to_rsf(idx, block)];
    for b in block.blocks.values() {
        ast.append(&mut all_blocks_to_rsf(idx, b));
    }
    ast
}

fn all_registers_to_rsf(idx: &Indexes) -> Vec<ast::Register> {
    idx.map.registers.iter().map(|r| register_to_rsf(idx, r)).collect()
}

pub fn convert(map: RegMap) -> Result<ast::Ast> {
    init_regexes();

    let mut indexes =
        Indexes { map, registers: BTreeMap::new(), blocks: BTreeMap::new() };
    let registers =
        indexes.map.registers.iter().map(|r| r.ref_name.clone()).collect();
    let mut block_tree = build_block_tree(&indexes.map)?;

    let mut ab = ArrayBuilderState::new();
    build_arrays(&mut ab, &mut block_tree);

    let mut blocks = BTreeSet::<String>::new();
    all_block_names(&block_tree, &mut blocks);
    indexes.registers = uniquify(registers);
    indexes.blocks = uniquify(blocks.into_iter().collect());
    println!("{:#?}", indexes.blocks);
    {
        // The registers and blocks all live in a single flat namespace,
        // unless we break it into multiple files.  Make sure we have no
        // conflicts.
        let r =
            indexes.registers.values().cloned().collect::<BTreeSet<String>>();
        let b = indexes.blocks.values().cloned().collect::<BTreeSet<String>>();
        assert!(b.is_disjoint(&r));
    }

    let ast = ast::Ast {
        blocks: all_blocks_to_rsf(&indexes, &block_tree),
        registers: all_registers_to_rsf(&indexes),
        enums: Vec::new(),
        use_statements: Vec::new(),
    };
    Ok(ast)
}
