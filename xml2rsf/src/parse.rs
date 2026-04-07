use std::fs::File;
use std::io::{BufReader, prelude::*};

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use convert_case::{Case, Casing};
use regex::Regex;

pub struct RegMap {
    pub address_maps: Vec<AddressMap>,
    pub registers: Vec<Register>,
}

#[derive(Debug)]
pub struct AddressMapEntry {
    pub low: u32,
    pub high: u32,
    #[allow(unused)]
    pub name: Option<String>,
    pub ref_name: Option<String>,
}

fn num_parse(s: &str) -> Result<u32> {
    if s.starts_with("0x") {
        u32::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16)
    } else {
        #[allow(clippy::from_str_radix_10)]
        u32::from_str_radix(s, 10)
    }
    .map_err(|e| anyhow!("parsing {s}: {e:?}"))
}

// Some registers put a range in the "offset" field (e.g., 0x0 - 0x800, +0x80)
// We only want the starting offset.
fn hex_range_parse(s: &str) -> Result<u32> {
    let end = s.find(" ").unwrap_or(s.len());
    num_parse(&s[0..end])
}

impl TryFrom<&RawNode> for AddressMapEntry {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        Ok(AddressMapEntry {
            low: num_parse(&node.get_child_value("addressLow")?)?,
            high: num_parse(&node.get_child_value("addressHigh")?)?,
            name: node.get_child_value("instanceName").ok(),
            ref_name: node.get_child_value("referenceName").ok(),
        })
    }
}

#[derive(Debug)]
pub struct AddressMap {
    pub entries: Vec<AddressMapEntry>,
}

impl TryFrom<&RawNode> for AddressMap {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        match node {
            RawNode::Container { children, .. } => {
                let all = children.len();
                let entries = children
                    .iter()
                    .map(AddressMapEntry::try_from)
                    .collect::<Result<Vec<AddressMapEntry>, anyhow::Error>>(
                )?;

                if entries.len() == all {
                    Ok(AddressMap {
                        entries: entries
                            .into_iter()
                            .filter(|e| match &e.ref_name {
                                Some(r) => {
                                    !r.starts_with("jbay_reg.eth400g_")
                                        || r.starts_with("jbay_reg.eth400g_p1.")
                                }
                                None => false,
                            })
                            .collect(),
                    })
                } else {
                    Err(anyhow!("addressMap contains non-entry"))
                }
            }
            _ => Err(anyhow!("addressMap isn't a container")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessMode {
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

#[derive(Debug, Clone)]
pub struct Register {
    pub ref_name: String,
    pub id: String,
    #[allow(unused)]
    pub title: String,
    #[allow(unused)]
    pub access: AccessMode,
    #[allow(unused)]
    pub offset: u32,
    pub reset_value: Option<u32>,
    #[allow(unused)]
    pub reset_mask: Option<u32>,
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
                    .map(Bitfield::try_from)
                    .collect::<Result<Vec<Bitfield>, anyhow::Error>>()?
            } else {
                Vec::new()
            }
        };

        Ok(Register {
            ref_name: node.get_child_value("referenceName")?,
            id: node.get_child_value("identifier")?,
            title: node.get_child_value("title")?,
            offset: hex_range_parse(&node.get_child_value("offset")?)?,
            reset_value: node
                .get_child_value("resetValue")
                .map(|n| num_parse(&n).unwrap())
                .ok(),
            reset_mask: node
                .get_child_value("resetMask")
                .map(|n| num_parse(&n).unwrap())
                .ok(),
            access: access_parse(&node.get_child_value("addressedAccess")?)?,
            bitfields,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Bitfield {
    pub id: String,
    pub access: AccessMode,
    pub lsb: u8,
    pub msb: u8,
}

impl TryFrom<&RawNode> for Bitfield {
    type Error = anyhow::Error;
    fn try_from(node: &RawNode) -> std::result::Result<Self, Self::Error> {
        Ok(Bitfield {
            id: node.get_child_value("id").unwrap_or(
                node.get_child_value("identifier")?.to_case(Case::Snake),
            ),
            access: access_parse(&node.get_child_value("access")?)?,
            lsb: num_parse(&node.get_child_value("lsb")?)? as u8,
            msb: num_parse(&node.get_child_value("msb")?)? as u8,
        })
    }
}

#[derive(Debug)]
pub enum RawNode {
    Container { name: String, children: Vec<RawNode> },
    Value { name: String, value: String },
}

impl RawNode {
    pub fn name(&self) -> &String {
        match self {
            RawNode::Value { name, .. } => name,
            RawNode::Container { name, .. } => name,
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
                if let Some(c) = children.iter().find(|c| match c {
                    RawNode::Container { name, .. } => name == find_name,
                    RawNode::Value { .. } => false,
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
    pub fn get_child_ref(&self) -> Result<&Vec<RawNode>> {
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
        self.line_no += 1;
        let line_no = self.line_no;

        match classified {
            LineType::Text(value) => {
                Ok(Some(RawNode::Value { name: String::new(), value }))
            }
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
                if x != parsing {
                    bail!("found {} ending {} at line {}", x, parsing, line_no);
                }
                Ok(None)
            }
        }
    }

    pub fn validate_header(&mut self) -> Result<()> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        self.line_no += 1;
        if line.trim() == "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>" {
            Ok(())
        } else {
            Err(anyhow!("invalid xml header"))
        }
    }
}

fn convert(raw: RawNode) -> Result<RegMap> {
    let definitions = match raw.find_descendant_container("definitions") {
        Some(RawNode::Container { children, .. }) => children,
        _ => bail!("no definitions found"),
    };

    let mut address_maps = Vec::new();
    let mut registers = Vec::new();
    for d in definitions.iter() {
        match d.get_child_value("referenceType").unwrap().as_str() {
            "addressmap" => {
                // Some sections tagged as "addressMap" don't actually
                // contain any map entries.
                if let Some(root) = d.find_descendant_container("addressMap") {
                    address_maps.push(AddressMap::try_from(root).map_err(|e| {
                    anyhow!( "failed to convert {root:#?} to an addressmap: {e:?}")})?)
                }
            }
            "register" => {
                let r = Register::try_from(d).map_err(|e| {
                    anyhow!("failed to convert {d:#?} to a register: {e:?}")
                })?;
                if !r.ref_name.starts_with("jbay_reg.eth400g_")
                    || r.ref_name.starts_with("jbay_reg.eth400g_p1.")
                {
                    registers.push(r);
                }
            }
            _ => {}
        }
    }

    Ok(RegMap { address_maps, registers })
}

pub fn parse_xml(xml: &String) -> Result<RegMap> {
    let file = File::open(xml)?;
    let reader = BufReader::new(file);
    let mut parser = Parser::new(reader);

    parser.validate_header()?;
    if let Some(raw) = parser.parse("csrData")? {
        convert(raw)
    } else {
        bail!("unrecognized register map");
    }
}
