use std::fs::File;
use std::io::{prelude::*, BufReader};

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use regex::Regex;

#[derive(Debug)]
enum Node {
    AddressMap(AddressMap),
    AddressMapEntry(AddressMapEntry),
    Register,
    Group(Group),
    Other(String),
    Raw(RawNode),
}

#[derive(Debug)]
struct AddressMapEntry {
    pub low: u32,
    pub high: u32,
    pub name: Option<String>,
    pub ref_name: Option<String>,
}

fn hex_parse(s: &str) -> Result<u32> {
    u32::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16)
        .map_err(|e| anyhow!("parsing {s}: {e:?}"))
}

// Some registers put a range in the "offset" field (e.g., 0x0 - 0x800, +0x80)
// We only want the starting offset.
fn hex_range_parse(s: &str) -> Result<u32> {
    let end = s.find(" ").unwrap_or_else(|| s.len());
    hex_parse(&s[0..end])
}

impl TryFrom<&RawNode> for AddressMapEntry {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        Ok(AddressMapEntry {
            low: hex_parse(&node.get_child_value("addressLow")?)?,
            high: hex_parse(&node.get_child_value("addressHigh")?)?,
            name: node.get_child_value("instanceName").ok(),
            ref_name: node.get_child_value("referenceName").ok(),
        })
    }
}

#[derive(Debug)]
struct AddressMap {
    entries: Vec<AddressMapEntry>,
}

impl TryFrom<&RawNode> for AddressMap {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        match node {
            RawNode::Container { children, .. } => {
                let all = children.len();
                let entries = children
                    .into_iter()
                    .map(|c| AddressMapEntry::try_from(c))
                    .collect::<Result<Vec<AddressMapEntry>, anyhow::Error>>()?;
                if entries.len() == all {
                    Ok(AddressMap { entries })
                } else {
                    Err(anyhow!("addressMap contains non-entry"))
                }
            }
            _ => Err(anyhow!("addressMap isn't a container")),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    ReadWrite1Clear,
}

fn access_parse(s: &str) -> Result<AccessMode> {
    match s.to_lowercase().as_str() {
        "r" | "rc" => Ok(AccessMode::ReadOnly),
        "w" => Ok(AccessMode::WriteOnly),
        "rw" | "r/w" => Ok(AccessMode::ReadWrite),
        "r/w1c" => Ok(AccessMode::ReadWrite1Clear),
        x => bail!("unrecognized access type: {}", x),
    }
}

#[derive(Debug)]
struct Register {
    pub ref_name: String,
    pub title: String,
    pub access: AccessMode,
    pub offset: u32,
    pub reset_value: u32,
    pub reset_mask: u32,
    pub bitfields: Vec<Bitfield>,
}

impl TryFrom<&RawNode> for Register {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        let bitfields_node = node
            .get_child_ref()?
            .iter()
            .find(|c| c.name().as_str() == "bitfields");
        let bitfields = {
            if let Some(node) = bitfields_node {
                node.get_child_ref()?
                    .iter()
                    .map(|c| Bitfield::try_from(c))
                    .collect::<Result<Vec<Bitfield>, anyhow::Error>>()?
            } else {
                Vec::new()
            }
        };

        Ok(Register {
            ref_name: node.get_child_value("referenceName")?,
            title: node.get_child_value("title")?,
            offset: hex_range_parse(&node.get_child_value("offset")?)?,
            reset_value: hex_parse(
                &node
                    .get_child_value("resetValue")
                    .unwrap_or("0x0".to_string()),
            )?,
            reset_mask: hex_parse(
                &node
                    .get_child_value("resetMask")
                    .unwrap_or("0xffffffff".to_string()),
            )?,
            access: access_parse(&node.get_child_value("addressedAccess")?)?,
            bitfields,
        })
    }
}

#[derive(Debug)]
struct Bitfield {
    pub id: String,
    pub access: AccessMode,
    pub lsb: u8,
    pub msb: u8,
}

impl TryFrom<&RawNode> for Bitfield {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        Ok(Bitfield {
            id: node
                .get_child_value("id")
                .unwrap_or(node.get_child_value("identifier")?),
            access: access_parse(&node.get_child_value("access")?)?,
            lsb: hex_parse(&node.get_child_value("lsb")?)? as u8,
            msb: hex_parse(&node.get_child_value("msb")?)? as u8,
        })
    }
}

#[derive(Debug)]
struct Group {
    pub ref_name: String,
    pub offset: u32,
}

impl TryFrom<&RawNode> for Group {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        Ok(Group {
            offset: hex_range_parse(&node.get_child_value("offset")?)?,
            ref_name: node.get_child_value("referenceName")?,
        })
    }
}

#[derive(Debug)]
enum RawNode {
    Container {
        name: String,
        children: Vec<RawNode>,
    },
    Value {
        name: String,
        value: String,
    },
}

impl RawNode {
    pub fn name<'a>(&self) -> &String {
        match self {
            RawNode::Value { name, .. } => return &name,
            RawNode::Container { name, .. } => return &name,
        }
    }

    pub fn is_container(&self) -> bool {
        match self {
            RawNode::Value { .. } => false,
            RawNode::Container { .. } => true,
        }
    }

    // Do a breadth-first search of the Container children of this node,
    // looking for the first Container instance of the given name.  Return a
    // reference to the Container.
    pub fn find_descendant_container<'a>(
        &'a self,
        find_name: &str,
    ) -> Option<&'a RawNode> {
        match self {
            RawNode::Value { .. } => None,
            RawNode::Container { children, .. } => {
                if let Some(c) = children.iter().find_map(|c| match c {
                    RawNode::Container { name, .. } if name == find_name => {
                        Some(c)
                    }
                    _ => None,
                }) {
                    Some(c)
                } else {
                    children
                        .iter()
                        .find_map(|c| c.find_descendant_container(find_name))
                }
            }
        }
    }

    // Get a reference to a node's children.
    pub fn get_child_ref<'a>(&'a self) -> Result<&'a Vec<RawNode>> {
        match self {
            RawNode::Value { .. } => bail!("node is not a container"),
            RawNode::Container { children, .. } => Ok(children),
        }
    }

    // Look inside a Container for a Value node of the given name, and return
    // the corresponding value.
    pub fn get_child_value(&self, n: &str) -> Result<String> {
        self.get_child_ref()?
            .iter()
            .find_map(|c| match c {
                RawNode::Value { name, value } if name.as_str() == n => {
                    Some(value.clone())
                }
                _ => None,
            })
            .ok_or(anyhow!("{} not found", n))
    }
}

#[derive(Debug)]
enum LineType {
    Head(String),
    Full(String, String),
    Tail(String),
    Text(String),
}

struct Parser<R: std::io::Read> {
    line_no: u32,
    reader: BufReader<R>,
    re: Regex,
}

impl<R> Parser<R>
where
    R: std::io::Read,
{
    pub fn new(reader: BufReader<R>) -> Self {
        Parser {
            line_no: 0,
            reader,
            re: Regex::new(r"\s*(<csr:([^>]*)>)?([^<]*)(</csr:([^>]*)>)?\s*")
                .unwrap(),
        }
    }

    fn classify_line(&self, line: &str) -> Result<LineType> {
        let c = self.re.captures(line).unwrap();
        let tag1 = c.get(2).map(|m| m.as_str());
        let body = c.get(3).unwrap().as_str();
        let tag2 = c.get(5).map(|m| m.as_str());

        match (tag1, tag2) {
            (Some(tag1), Some(tag2)) => {
                if tag1 != tag2 {
                    bail!(
                        "line {}: mismatched tags: {} and {}",
                        self.line_no,
                        tag1,
                        tag2
                    );
                }
                Ok(LineType::Full(tag1.to_string(), body.to_string()))
            }
            (Some(tag1), None) => Ok(LineType::Head(tag1.to_string())),
            (None, Some(tag2)) => Ok(LineType::Tail(tag2.to_string())),
            _ => Ok(LineType::Text(line.to_string())),
        }
    }

    fn parse(&mut self, parsing: &str) -> Result<Option<RawNode>> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        if line.is_empty() {
            return Ok(None);
        }

        let classified = self.classify_line(&line)?;
        self.line_no = self.line_no + 1;
        let line_no = self.line_no;

        match classified {
            LineType::Text(value) => Ok(Some(RawNode::Value {
                name: String::new(),
                value,
            })),
            LineType::Full(name, value) => {
                Ok(Some(RawNode::Value { name, value }))
            }
            LineType::Head(name) => {
                let mut children = Vec::new();
                loop {
                    match self.parse(&name) {
                        Ok(Some(c)) => {
                            if !c.name().is_empty() {
                                children.push(c)
                            }
                        }
                        Ok(None) => {
                            // These just seem to be registering the names of
                            // the definitions to follow, and don't contain any
                            // useful information.
                            if name == "references" {
                                children = Vec::new();
                            }
                            return Ok(Some(RawNode::Container {
                                name: name.clone(),
                                children,
                            }));
                        }
                        Err(e) => {
                            eprintln!("{e:?}");
                            return Err(anyhow!(
                                "failed to parse {} at {}: {:?}",
                                name,
                                line_no,
                                e
                            ));
                        }
                    }
                }
            }
            LineType::Tail(x) => {
                if &x != parsing {
                    bail!("found {} ending {} at line {}", x, parsing, line_no);
                }
                Ok(None)
            }
        }
    }

    pub fn validate_header(&mut self) -> Result<()> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        self.line_no = self.line_no + 1;
        if line.trim() == "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>" {
            Ok(())
        } else {
            Err(anyhow!("invalid xml header"))
        }
    }
}

fn convert(raw: RawNode) -> Result<Node> {
    let definitions = match raw.find_descendant_container("definitions") {
        Some(RawNode::Container { children, .. }) => children,
        _ => bail!("no definitions found"),
    };

    let mut maps = Vec::new();
    let mut groups = Vec::new();
    let mut registers = Vec::new();
    for d in definitions.into_iter() {
        match d.get_child_value("referenceType").unwrap().as_str() {
            "addressmap" => {
                // Some sections tagged as "addressMap" don't actually
                // contain any map entries.
                if let Some(root) = d.find_descendant_container("addressMap") {
                    maps.push(AddressMap::try_from(root).map_err(|e| {
                    anyhow!( "failed to convert {root:#?} to an addressmap: {e:?}")}))
                }
            }
            "register" => {
                registers.push(Register::try_from(d).map_err(|e| {
                    anyhow!("failed to convert {d:#?} to a register: {e:?}",)
                })?)
            }
            "group" => groups.push(Group::try_from(d).map_err(|e| {
                anyhow!("failed to convert {d:#?} to a group: {e:?}")
            })?),
            _ => {}
        }
    }
    println!("maps: {maps:#?}");
    println!("registers: {registers:#?}");
    println!("groups: {groups:#?}");

    /*
    let node = if name == "addressMapEntry" {
        match AddressMapEntry::try_from(raw) {
            Ok(entry) => Node::AddressMapEntry(entry),
            Err(e) => {
                panic!("invalid addressMapEntry at {}: {:?}", self.line_no, e)
            }
        }
    } else if name == "addressMap" {
        match AddressMap::try_from(raw) {
            Ok(map) => Node::AddressMap(map),
            Err(e) => panic!("invalid addressMap at {}: {:?}", self.line_no, e),
        }
    } else {
    };
    */

    Ok(Node::Other("foo".to_string()))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        bail!("usage: {} <xml defs>", args[0]);
    }

    let file = File::open(&args[1])?;
    let reader = BufReader::new(file);
    let mut parser = Parser::new(reader);

    parser.validate_header()?;
    if let Some(raw) = parser.parse("csrData")? {
        convert(raw)?;
    }

    Ok(())
}
