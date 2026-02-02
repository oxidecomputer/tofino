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

#[derive(Debug)]
struct AddressMap {
    entries: Vec<AddressMapEntry>,
}

fn hex_parse(s: &str) -> Result<u32> {
    u32::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16)
        .map_err(|e| e.into())
}

impl TryFrom<RawNode> for AddressMapEntry {
    type Error = anyhow::Error;
    fn try_from(node: RawNode) -> std::result::Result<Self, Self::Error> {
        Ok(AddressMapEntry {
            low: hex_parse(&node.get_child_value("addressLow")?)?,
            high: hex_parse(&node.get_child_value("addressHigh")?)?,
            name: node.get_child_value("instanceName").ok(),
            ref_name: node.get_child_value("referenceName").ok(),
        })
    }
}

impl TryFrom<RawNode> for AddressMap {
    type Error = anyhow::Error;
    fn try_from(node: RawNode) -> std::result::Result<Self, Self::Error> {
        match node {
            RawNode::Container { children, .. } => {
                let all = children.len();
                let entries: Vec<AddressMapEntry> = children
                    .into_iter()
                    .filter_map(|c| match c {
                        Node::AddressMapEntry(a) => Some(a),
                        _ => None,
                    })
                    .collect();
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

#[derive(Debug)]
enum RawNode {
    Container { name: String, children: Vec<Node> },
    Value { name: String, value: String },
}

impl RawNode {
    pub fn get_child_value(&self, n: &str) -> Result<String> {
        match self {
            RawNode::Value { .. } => bail!("node is not a container"),
            RawNode::Container { children, .. } => {
                children.iter().find_map(|c| match c {
                    Node::Raw(RawNode::Value { name, value })
                        if name.as_str() == n =>
                    {
                        Some(value.clone())
                    }
                    _ => None,
                })
            }
        }
        .ok_or(anyhow!("{} not found", n))
    }
}

#[derive(Debug)]
enum LineType {
    Head(String),
    Full(String, String),
    Tail(String),
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
        let Some(c) = self.re.captures(line) else {
            bail!("bad line: {line}");
        };
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
            _ => Err(anyhow!("line {}: bad line: {}", self.line_no, line)),
        }
    }

    fn parse(&mut self) -> Result<Option<Node>> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        if line.is_empty() {
            return Ok(None);
        }

        let classified = self.classify_line(&line)?;
        self.line_no = self.line_no + 1;
        match classified {
            LineType::Full(name, value) => {
                Ok(Some(Node::Raw(RawNode::Value { name, value })))
            }
            LineType::Head(name) => {
                let mut children = Vec::new();
                loop {
                    match self.parse() {
                        Ok(Some(c)) => children.push(c),
                        Ok(None) => {
                            let raw = RawNode::Container {
                                name: name.clone(),
                                children,
                            };
                            let node = if name == "addressMapEntry" {
                                match AddressMapEntry::try_from(raw) {
                                    Ok(entry) => Node::AddressMapEntry(entry),
                                    Err(e) => panic!(
                                        "invalid addressMapEntry at {}: {:?}",
                                        self.line_no, e
                                    ),
                                }
                            } else if name == "addressMap" {
                                match AddressMap::try_from(raw) {
                                    Ok(map) => Node::AddressMap(map),
                                    Err(e) => panic!(
                                        "invalid addressMap at {}: {:?}",
                                        self.line_no, e
                                    ),
                                }
                            } else {
                                Node::Raw(raw)
                            };
                            return Ok(Some(node));
                        }
                        Err(e) => eprintln!("{e:?}"),
                    }
                }
            }
            LineType::Tail(_x) => Ok(None),
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        bail!("usage: {} <xml defs>", args[0]);
    }

    let file = File::open(&args[1])?;
    let reader = BufReader::new(file);
    let mut parser = Parser::new(reader);

    parser.validate_header()?;
    println!("{:#?}", parser.parse()?);

    Ok(())
}
