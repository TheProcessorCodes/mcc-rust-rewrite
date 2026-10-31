//! Turns the expression table back into Lisp-like source text.

use crate::types::{ValueType, type_name};
use crate::{NodeKind, ScriptData};

/// Lines longer than this are broken one child per line.
const WIDTH: usize = 110;

pub struct Decompiler<'a> {
    data: &'a ScriptData,
}

/// A form being laid out: a leaf, or a call with a head and children.
enum Form {
    Leaf(String),
    List(String, Vec<Form>),
}

impl Form {
    fn flat(&self) -> String {
        match self {
            Form::Leaf(s) => s.clone(),
            Form::List(head, kids) => {
                let mut s = format!("({head}");
                for k in kids {
                    s.push(' ');
                    s.push_str(&k.flat());
                }
                s.push(')');
                s
            }
        }
    }

    fn pretty(&self, indent: usize, out: &mut String) {
        let flat = self.flat();
        match self {
            Form::List(head, kids) if indent + flat.len() > WIDTH && !kids.is_empty() => {
                out.push('(');
                out.push_str(head);
                for k in kids {
                    out.push('\n');
                    out.push_str(&" ".repeat(indent + 2));
                    k.pretty(indent + 2, out);
                }
                out.push(')');
            }
            _ => out.push_str(&flat),
        }
    }
}

impl<'a> Decompiler<'a> {
    pub fn new(data: &'a ScriptData) -> Decompiler<'a> {
        Decompiler { data }
    }

    /// One script as a `(script ...)` form.
    pub fn script(&self, index: usize) -> String {
        let s = &self.data.scripts[index];
        let mut head = format!("script {} {} {}", s.kind.name(), type_name(s.return_type), s.name);
        if !s.parameters.is_empty() {
            let params: Vec<String> = s
                .parameters
                .iter()
                .map(|p| format!("({} {})", type_name(p.value_type), p.name))
                .collect();
            head = format!("{head} ({})", params.join(" "));
        }
        // The body is a `begin`: print its children directly.
        let body = match self.form(s.root, Some(index), 0) {
            Form::List(h, kids) if h == "begin" => kids,
            f => vec![f],
        };
        let mut out = format!("({head}");
        for k in body {
            out.push_str("\n  ");
            k.pretty(2, &mut out);
        }
        out.push(')');
        out
    }

    /// One expression, flat or broken over lines.
    pub fn expression(&self, node: u16) -> String {
        let mut out = String::new();
        self.form(node, None, 0).pretty(0, &mut out);
        out
    }

    /// A constant's text (no quotes for strings).
    pub fn constant(&self, node: u16) -> String {
        let Some(n) = self.data.node(node) else {
            return "<free>".into();
        };
        let text = || self.data.string(n.string).map(str::to_string);
        match n.value_type {
            ValueType::BOOLEAN => if n.as_bool() { "true" } else { "false" }.into(),
            ValueType::REAL => {
                let v = n.as_f32();
                if v == v.trunc() && v.abs() < 1e7 { format!("{v:.1}") } else { format!("{v}") }
            }
            ValueType::SHORT => n.as_i16().to_string(),
            ValueType::LONG => (n.value as i32).to_string(),
            ValueType::STRING => format!("\"{}\"", text().unwrap_or_default()),
            ValueType::VOID => "none".into(),
            ValueType::SCRIPT => self
                .data
                .scripts
                .get(n.as_i16() as usize)
                .map_or_else(|| text().unwrap_or_default(), |s| s.name.clone()),
            t if t.names_object() && n.as_i16() < 0 => "none".into(),
            _ => text().unwrap_or_else(|| format!("<{} {:#x}>", type_name(n.value_type), n.value)),
        }
    }

    fn form(&self, node: u16, script: Option<usize>, depth: usize) -> Form {
        let Some(n) = self.data.node(node) else {
            return Form::Leaf(format!("<free node {node}>"));
        };
        if depth > 200 {
            return Form::Leaf("<too deep>".into());
        }
        match n.kind() {
            NodeKind::Call | NodeKind::ScriptCall => {
                let kids = self.data.children(node);
                let head = match n.kind() {
                    NodeKind::ScriptCall => self
                        .data
                        .scripts
                        .get(n.opcode as usize)
                        .map_or_else(|| format!("<script {}>", n.opcode), |s| s.name.clone()),
                    _ => kids
                        .first()
                        .and_then(|h| self.data.node(*h))
                        .and_then(|h| self.data.string(h.string))
                        .unwrap_or("<function>")
                        .to_string(),
                };
                let mut args: Vec<Form> = kids
                    .iter()
                    .skip(1)
                    .map(|k| self.form(*k, script, depth + 1))
                    .collect();
                // `(if c a)`: the compiler fills in a void `else`.
                if head == "if"
                    && args.len() == 3
                    && kids.last().and_then(|k| self.data.node(*k)).is_some_and(|k| {
                        k.kind() == NodeKind::Constant && k.value_type == ValueType::VOID
                    })
                {
                    args.pop();
                }
                Form::List(head, args)
            }
            NodeKind::Global => {
                let index = n.value as u16;
                if index & 0x8000 == 0 {
                    Form::Leaf(
                        self.data
                            .globals
                            .get(index as usize)
                            .map_or_else(|| format!("<global {index}>"), |g| g.name.clone()),
                    )
                } else {
                    // An engine global: only its name is stored.
                    Form::Leaf(self.data.string(n.string).unwrap_or("<engine global>").to_string())
                }
            }
            NodeKind::Parameter => {
                let p = script
                    .and_then(|s| self.data.scripts.get(s))
                    .and_then(|s| s.parameters.get(n.value as usize))
                    .map(|p| p.name.clone());
                Form::Leaf(p.or_else(|| self.data.string(n.string).map(str::to_string)).unwrap_or_else(|| "<parameter>".into()))
            }
            NodeKind::Constant => Form::Leaf(self.constant(node)),
        }
    }
}
