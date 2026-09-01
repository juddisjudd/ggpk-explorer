//! GGG's object definition DSL (`.ao/.aoc`, `.ot/.otc`, `.it`, `.act`, `.epk`):
//! a `version`/`extends` header followed by named component blocks of
//! `key = value` lines. Blocks whose body is JSON (`RenderPasses { "passes": … }`)
//! and single-quoted JSON values (`animations = '[…]'`) are parsed into values.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ObjectFile {
    pub version: Option<u32>,
    pub extends: Option<String>,
    pub is_abstract: bool,
    pub includes: Vec<String>,
    /// Top-level statements that are not blocks (`movement_speed 330`, `ParticleEffect "blade" "x.pet"`).
    pub props: Vec<Prop>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Component {
    pub name: String,
    /// Words after the name on the block's opening line (`animated_object "x.ao"`, `stance Crossbow`).
    pub args: Vec<String>,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
    /// The block body when it was a JSON document rather than statements.
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Prop {
    pub key: String,
    /// Value text; a lone quoted string is stored without its quotes.
    pub value: String,
    /// The value parsed as JSON when it was one (single-quoted or brace-delimited).
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    Raw(String),
    Eq,
    LBrace,
    RBrace,
    Newline,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(text: &str) -> Self {
        Self { chars: text.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next_tok(&mut self) -> Option<Tok> {
        loop {
            let c = self.peek()?;
            match c {
                '\n' => {
                    self.pos += 1;
                    return Some(Tok::Newline);
                }
                ';' | ',' => {
                    self.pos += 1;
                    return Some(Tok::Newline);
                }
                c if c.is_whitespace() => self.pos += 1,
                '/' if self.chars.get(self.pos + 1) == Some(&'/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                '{' => {
                    self.pos += 1;
                    return Some(Tok::LBrace);
                }
                '}' => {
                    self.pos += 1;
                    return Some(Tok::RBrace);
                }
                '=' => {
                    self.pos += 1;
                    return Some(Tok::Eq);
                }
                '"' => {
                    self.pos += 1;
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == '\\' {
                            if let Some(n) = self.peek() {
                                s.push(n);
                                self.pos += 1;
                            }
                            continue;
                        }
                        if c == '"' {
                            break;
                        }
                        s.push(c);
                    }
                    return Some(Tok::Str(s));
                }
                '\'' => {
                    self.pos += 1;
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        if c == '\'' {
                            break;
                        }
                        s.push(c);
                    }
                    return Some(Tok::Raw(s));
                }
                _ => {
                    let mut s = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_whitespace() || matches!(c, '{' | '}' | '=' | '"' | '\'' | ';' | ',') {
                            break;
                        }
                        s.push(c);
                        self.pos += 1;
                    }
                    return Some(Tok::Word(s));
                }
            }
        }
    }

    /// Skips newlines and reports whether the next token is `{` without consuming it.
    fn brace_follows(&mut self) -> bool {
        let save = self.pos;
        loop {
            match self.next_tok() {
                Some(Tok::Newline) => continue,
                Some(Tok::LBrace) => {
                    self.pos = save;
                    return true;
                }
                _ => {
                    self.pos = save;
                    return false;
                }
            }
        }
    }

    fn body_is_json(&self) -> bool {
        self.chars[self.pos..].iter().find(|c| !c.is_whitespace()) == Some(&'"')
    }

    /// Text from just after an opening brace to its matching close, braces included.
    fn capture_balanced(&mut self) -> String {
        let mut depth = 1usize;
        let mut out = String::from("{");
        let mut in_str = false;
        while let Some(c) = self.peek() {
            self.pos += 1;
            out.push(c);
            if in_str {
                if c == '\\' {
                    if let Some(n) = self.peek() {
                        out.push(n);
                        self.pos += 1;
                    }
                } else if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Word(w) => w.clone(),
        Tok::Str(s) => format!("\"{}\"", s),
        Tok::Raw(s) => format!("'{}'", s),
        Tok::Eq => "=".into(),
        Tok::LBrace => "{".into(),
        Tok::RBrace => "}".into(),
        Tok::Newline => String::new(),
    }
}

fn value_prop(key: String, toks: &[Tok]) -> Prop {
    match toks {
        [Tok::Str(s)] => Prop { key, value: s.clone(), json: None },
        [Tok::Raw(s)] => {
            let json = serde_json::from_str(s.trim()).ok();
            Prop { key, value: s.trim().to_string(), json }
        }
        _ => Prop { key, value: toks.iter().map(tok_text).collect::<Vec<_>>().join(" "), json: None },
    }
}

struct Block {
    props: Vec<Prop>,
    components: Vec<Component>,
}

fn parse_block(lex: &mut Lexer, file: &mut ObjectFile, top: bool) -> Block {
    let mut block = Block { props: Vec::new(), components: Vec::new() };
    while let Some(first) = lex.next_tok() {
        match first {
            Tok::Newline => continue,
            Tok::RBrace => {
                if top {
                    continue;
                }
                break;
            }
            Tok::LBrace => {
                // Anonymous block (`.pet` emitters): keep its statements as a nameless component.
                let inner = parse_block(lex, file, false);
                block.components.push(Component { name: String::new(), args: Vec::new(), props: inner.props, children: inner.components, json: None });
                continue;
            }
            _ => {}
        }
        let mut line = vec![first];
        let mut opens = false;
        loop {
            match lex.next_tok() {
                None | Some(Tok::Newline) => break,
                Some(Tok::RBrace) => {
                    lex.pos -= 1;
                    break;
                }
                Some(Tok::LBrace) => {
                    opens = true;
                    break;
                }
                Some(t) => line.push(t),
            }
        }
        if !opens && line.iter().all(|t| matches!(t, Tok::Word(_) | Tok::Str(_))) && lex.brace_follows() {
            while !matches!(lex.next_tok(), Some(Tok::LBrace) | None) {}
            opens = true;
        }

        if opens {
            let eq_at = line.iter().position(|t| *t == Tok::Eq);
            if let Some(1) = eq_at {
                // `Functions { Name = { … } }`: a brace-delimited value.
                let raw = lex.capture_balanced();
                let key = tok_text(&line[0]);
                block.props.push(Prop { key, value: raw, json: None });
                continue;
            }
            let name = line.first().map(tok_text).unwrap_or_default();
            let args: Vec<String> = line.iter().skip(1).map(|t| match t {
                Tok::Str(s) => s.clone(),
                t => tok_text(t),
            }).collect();
            if lex.body_is_json() {
                let raw = lex.capture_balanced();
                let json = serde_json::from_str::<serde_json::Value>(&raw).ok();
                let props = if json.is_none() { vec![Prop { key: String::new(), value: raw, json: None }] } else { Vec::new() };
                block.components.push(Component { name, args, props, children: Vec::new(), json });
            } else {
                let inner = parse_block(lex, file, false);
                block.components.push(Component { name, args, props: inner.props, children: inner.components, json: None });
            }
            continue;
        }

        // Plain statement.
        if let Some(pos) = line.iter().position(|t| *t == Tok::Eq) {
            let key = line[..pos].iter().map(tok_text).collect::<Vec<_>>().join(" ");
            block.props.push(value_prop(key, &line[pos + 1..]));
            continue;
        }
        let key = tok_text(&line[0]);
        let rest = &line[1..];
        if top {
            match (line[0].clone(), rest) {
                (Tok::Word(w), [Tok::Word(v)]) if w == "version" => {
                    file.version = v.parse().ok();
                    continue;
                }
                (Tok::Word(w), [Tok::Str(s)]) if w == "extends" || w == "parent" => {
                    file.extends = Some(s.clone());
                    continue;
                }
                (Tok::Word(w), []) if w == "abstract" => {
                    file.is_abstract = true;
                    continue;
                }
                (Tok::Word(w), [Tok::Str(s)]) if w == "include" => {
                    file.includes.push(s.clone());
                    continue;
                }
                _ => {}
            }
        }
        if rest.is_empty() {
            block.props.push(Prop { key, value: String::new(), json: None });
        } else {
            block.props.push(value_prop(key, rest));
        }
    }
    block
}

pub fn parse(text: &str) -> ObjectFile {
    let mut file = ObjectFile::default();
    let mut lex = Lexer::new(text);
    let block = parse_block(&mut lex, &mut file, true);
    file.props = block.props;
    file.components = block.components;
    file
}

pub fn is_object_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [".ao", ".aoc", ".ot", ".otc", ".it", ".act", ".epk"].iter().any(|e| p.ends_with(e))
}

/// Path an `extends "…"` value points at: the game omits the extension, so borrow the child's.
pub fn resolve_extends(extends: &str, child_path: &str) -> String {
    let target = extends.replace('\\', "/");
    if target.rsplit('/').next().map(|f| f.contains('.')).unwrap_or(false) {
        return target;
    }
    let ext = child_path.rsplit('.').next().unwrap_or("ao");
    format!("{}.{}", target, ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ao_with_nested_client_block() {
        let doc = parse(
            "version 3\nextends \"Metadata/SkinHelmetArmour\"\n\nBaseAnimationEvents\n{\n}\n\nclient\n{\n\tSkinMesh\n\t{\n\t\tskin = \"Art/Models/X.sm\"\n\t}\n\tObjectHiding\n\t{\n\t    add_hide_parent_attachments = \"Face_Attach\"\n\t}\n}\n",
        );
        assert_eq!(doc.version, Some(3));
        assert_eq!(doc.extends.as_deref(), Some("Metadata/SkinHelmetArmour"));
        assert_eq!(doc.components.len(), 2);
        let client = &doc.components[1];
        assert_eq!(client.name, "client");
        assert_eq!(client.children.len(), 2);
        assert_eq!(client.children[0].props[0].key, "skin");
        assert_eq!(client.children[0].props[0].value, "Art/Models/X.sm");
    }

    #[test]
    fn parses_act_statements_and_stances() {
        let doc = parse(
            "parent \"Metadata/Characters/Character.act\"\nanimated_object \"Metadata/M/X.ao\"\n{\n\tIdle = \"idle_01\";\n\tEmerge = \"e_01\" effect_detached \"Metadata/E/s.ao\" \"release\";\n\tMelee = \"attack_01, attack_02\";\n\t\tthen \"sword_01\";\n\tstance Crossbow\n\t{\n\t\tEmerge = \"cb_01\" timeline \"Metadata/T/m.atl\";\n\t}\n}\nmovement_speed 330\naction_set minion\n",
        );
        assert_eq!(doc.extends.as_deref(), Some("Metadata/Characters/Character.act"));
        let ao = &doc.components[0];
        assert_eq!(ao.name, "animated_object");
        assert_eq!(ao.args, vec!["Metadata/M/X.ao"]);
        assert_eq!(ao.props[0].key, "Idle");
        assert_eq!(ao.props[1].value, "\"e_01\" effect_detached \"Metadata/E/s.ao\" \"release\"");
        assert_eq!(ao.props[3].key, "then");
        assert_eq!(ao.children[0].name, "stance");
        assert_eq!(ao.children[0].args, vec!["Crossbow"]);
        assert_eq!(doc.props.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), vec!["movement_speed", "action_set"]);
        assert_eq!(doc.props[0].value, "330");
    }

    #[test]
    fn parses_epk_json_block_and_particle_lines() {
        let doc = parse(
            "RenderPasses \n{\n    \"passes\": [\n        { \"is_main\": true, \"filename\": \"Metadata/x_pass0.mat\" }\n    ]\n}\nParticleEffect \"blade\" \"Metadata/E/shield.pet\"\n",
        );
        let rp = &doc.components[0];
        assert_eq!(rp.name, "RenderPasses");
        assert!(rp.json.as_ref().unwrap().get("passes").is_some());
        assert_eq!(doc.props[0].key, "ParticleEffect");
        assert_eq!(doc.props[0].value, "\"blade\" \"Metadata/E/shield.pet\"");
    }

    #[test]
    fn parses_functions_and_single_quoted_json() {
        let doc = parse(
            "Functions\n{\n\tKillUnique = { PlayAnimation( kill_01 ); }\n}\nclient\n{\n\tSoundEvents\n\t{\n\t\tanimations = '[ { \"name\": \"animate\", \"events\": [] } ]'\n\t}\n}\nBuffs {}\n",
        );
        let f = &doc.components[0];
        assert_eq!(f.props[0].key, "KillUnique");
        assert!(f.props[0].value.starts_with("{ PlayAnimation"));
        let se = &doc.components[1].children[0];
        assert!(se.props[0].json.as_ref().unwrap().is_array());
        assert_eq!(doc.components[2].name, "Buffs");
        assert!(doc.components[2].props.is_empty());
    }

    #[test]
    fn resolves_extends_with_child_extension() {
        assert_eq!(resolve_extends("Metadata/Parent", "Metadata/Items/x.it"), "Metadata/Parent.it");
        assert_eq!(resolve_extends("Metadata/Characters/Character.act", "a.act"), "Metadata/Characters/Character.act");
        assert!(is_object_path("Metadata/X.otc"));
        assert!(!is_object_path("Metadata/X.pet"));
    }
}
