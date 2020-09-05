//! The syntax tree.

use std::fmt::{self, Debug, Formatter};

use super::decoration::Decoration;
use super::span::{SpanVec, Spanned};
use super::tokens::is_ident;
use crate::color::RgbaColor;
use crate::compute::table::{SpannedEntry, Table};
use crate::compute::{TableValue, Value};
use crate::layout::Env;
use crate::length::Length;
use crate::DynFuture;

/// A collection of nodes which form a tree together with the nodes' children.
pub type SyntaxTree = SpanVec<SyntaxNode>;

/// A syntax node, which encompasses a single logical entity of parsed source
/// code.
#[derive(Debug, Clone, PartialEq)]
pub enum SyntaxNode {
    /// Whitespace containing less than two newlines.
    Spacing,
    /// A forced line break.
    Linebreak,
    /// A paragraph break.
    Parbreak,
    /// Italics were enabled / disabled.
    ToggleItalic,
    /// Bolder was enabled / disabled.
    ToggleBolder,
    /// Plain text.
    Text(String),
    /// Section headings.
    Heading(Heading),
    /// Lines of raw text.
    Raw(Vec<String>),
    /// An optionally highlighted (multi-line) code block.
    Code(Code),
    /// A function call.
    Call(Call),
}

/// A section heading.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    /// The section depth (how many hashtags minus 1).
    pub level: Spanned<u8>,
    pub tree: SyntaxTree,
}

/// A code block.
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub lang: Option<Spanned<Ident>>,
    pub lines: Vec<String>,
    pub block: bool,
}

/// An expression.
#[derive(Clone, PartialEq)]
pub enum Expr {
    /// An identifier: `ident`.
    Ident(Ident),
    /// A string: `"string"`.
    Str(String),
    /// A boolean: `true, false`.
    Bool(bool),
    /// A number: `1.2, 200%`.
    Number(f64),
    /// A length: `2cm, 5.2in`.
    Length(Length),
    /// A color value with alpha channel: `#f79143ff`.
    Color(RgbaColor),
    /// A table expression: `(false, 12cm, greeting="hi")`.
    Table(TableExpr),
    /// A syntax tree containing typesetting content.
    Tree(SyntaxTree),
    /// A function call: `cmyk(37.7, 0, 3.9, 1.1)`.
    Call(Call),
    /// An operation that negates the contained expression.
    Neg(Box<Spanned<Expr>>),
    /// An operation that adds the contained expressions.
    Add(Box<Spanned<Expr>>, Box<Spanned<Expr>>),
    /// An operation that subtracts the contained expressions.
    Sub(Box<Spanned<Expr>>, Box<Spanned<Expr>>),
    /// An operation that multiplies the contained expressions.
    Mul(Box<Spanned<Expr>>, Box<Spanned<Expr>>),
    /// An operation that divides the contained expressions.
    Div(Box<Spanned<Expr>>, Box<Spanned<Expr>>),
}

impl Expr {
    /// A natural-language name of the type of this expression, e.g.
    /// "identifier".
    pub fn name(&self) -> &'static str {
        use Expr::*;
        match self {
            Ident(_) => "identifier",
            Str(_) => "string",
            Bool(_) => "bool",
            Number(_) => "number",
            Length(_) => "length",
            Color(_) => "color",
            Table(_) => "table",
            Tree(_) => "syntax tree",
            Call(_) => "function call",
            Neg(_) => "negation",
            Add(_, _) => "addition",
            Sub(_, _) => "subtraction",
            Mul(_, _) => "multiplication",
            Div(_, _) => "division",
        }
    }

    /// Evaluate the expression to a value.
    pub async fn eval(&self, env: &mut Env) -> Value {
        use Expr::*;
        match self {
            Ident(i) => Value::Ident(i.clone()),
            Str(s) => Value::Str(s.clone()),
            &Bool(b) => Value::Bool(b),
            &Number(n) => Value::Number(n),
            &Length(s) => Value::Length(s),
            &Color(c) => Value::Color(c),
            Table(t) => Value::Table(t.eval(env).await),
            Tree(t) => Value::Tree(t.clone()),
            Call(call) => call.eval(env).await,
            Neg(_) => todo!("eval neg"),
            Add(_, _) => todo!("eval add"),
            Sub(_, _) => todo!("eval sub"),
            Mul(_, _) => todo!("eval mul"),
            Div(_, _) => todo!("eval div"),
        }
    }
}

impl Debug for Expr {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use Expr::*;
        match self {
            Ident(i) => i.fmt(f),
            Str(s) => s.fmt(f),
            Bool(b) => b.fmt(f),
            Number(n) => n.fmt(f),
            Length(s) => s.fmt(f),
            Color(c) => c.fmt(f),
            Table(t) => t.fmt(f),
            Tree(t) => t.fmt(f),
            Call(c) => c.fmt(f),
            Neg(e) => write!(f, "-{:?}", e),
            Add(a, b) => write!(f, "({:?} + {:?})", a, b),
            Sub(a, b) => write!(f, "({:?} - {:?})", a, b),
            Mul(a, b) => write!(f, "({:?} * {:?})", a, b),
            Div(a, b) => write!(f, "({:?} / {:?})", a, b),
        }
    }
}

/// An identifier as defined by unicode with a few extra permissible characters.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Ident(pub String);

impl Ident {
    /// Create a new identifier from a string checking that it is a valid.
    pub fn new(ident: impl AsRef<str> + Into<String>) -> Option<Self> {
        if is_ident(ident.as_ref()) {
            Some(Self(ident.into()))
        } else {
            None
        }
    }

    /// Return a reference to the underlying string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for Ident {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "`{}`", self.0)
    }
}

/// A table of expressions.
///
/// # Example
/// ```typst
/// (false, 12cm, greeting="hi")
/// ```
pub type TableExpr = Table<SpannedEntry<Expr>>;

impl TableExpr {
    /// Evaluate the table expression to a table value.
    pub fn eval<'a>(&'a self, env: &'a mut Env) -> DynFuture<'a, TableValue> {
        Box::pin(async move {
            let mut table = TableValue::new();

            for (key, entry) in self.iter() {
                let val = entry.val.v.eval(env).await;
                let spanned = Spanned::new(val, entry.val.span);
                let entry = SpannedEntry::new(entry.key, spanned);
                table.insert(key, entry);
            }

            table
        })
    }
}

/// An invocation of a function.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub name: Spanned<Ident>,
    pub args: TableExpr,
}

impl Call {
    /// Evaluate the call expression to a value.
    pub async fn eval(&self, env: &mut Env) -> Value {
        let name = self.name.v.as_str();
        let span = self.name.span;
        let args = self.args.eval(env).await;

        if let Some(func) = env.state.scope.func(name) {
            (*func.clone())(span, args, env).await
        } else {
            if !name.is_empty() {
                error!(@env.f, span, "unknown function");
                env.f.decorations.push(Spanned::new(Decoration::Unresolved, span));
            }
            Value::Table(args)
        }
    }
}
