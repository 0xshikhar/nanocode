use std::{
    borrow::Cow,
    fmt::{Display, Formatter, Write},
};

use serde::{
    Serialize,
    ser::{SerializeMap, SerializeSeq},
};

use crate::{ErrorKind, InstanceLocation, ValidationError, util::*};

impl<'s> ValidationError<'s, '_> {
    const fn absolute_keyword_location(&self) -> AbsoluteKeywordLocation<'s> {
        if let ErrorKind::Reference { url, .. } = &self.kind {
            AbsoluteKeywordLocation {
                schema_url: url,
                keyword_path: None,
            }
        } else {
            AbsoluteKeywordLocation {
                schema_url: self.schema_url,
                keyword_path: self.kind.keyword_path(),
            }
        }
    }

    const fn skip(&self) -> bool {
        self.causes.len() == 1 && matches!(self.kind, ErrorKind::Reference { .. })
    }

    pub const fn flag_output(&self) -> FlagOutput {
        FlagOutput { valid: false }
    }

    pub fn basic_output(&self) -> OutputUnit<'_, '_, '_> {
        let mut outputs = vec![];

        let mut in_ref = InRef::default();
        let mut kw_loc = KeywordLocation::default();
        for node in DfsIterator::new(self) {
            match node {
                DfsItem::Pre(e) => {
                    in_ref.pre(e);
                    kw_loc.pre(e);
                    if e.skip() || matches!(e.kind, ErrorKind::Schema { .. }) {
                        continue;
                    }
                    let absolute_keyword_location = if in_ref.get() {
                        Some(e.absolute_keyword_location())
                    } else {
                        None
                    };
                    outputs.push(OutputUnit {
                        valid: false,
                        keyword_location: kw_loc.get(e),
                        absolute_keyword_location,
                        instance_location: &e.instance_location,
                        error: OutputError::Leaf(&e.kind),
                    });
                }
                DfsItem::Post(e) => {
                    in_ref.post();
                    kw_loc.post();
                    if e.skip() || matches!(e.kind, ErrorKind::Schema { .. }) {
                        continue;
                    }
                }
            }
        }

        let error = if outputs.is_empty() {
            OutputError::Leaf(&self.kind)
        } else {
            OutputError::Branch(outputs)
        };
        OutputUnit {
            valid: false,
            keyword_location: String::new(),
            absolute_keyword_location: None,
            instance_location: &self.instance_location,
            error,
        }
    }

    pub fn detailed_output(&self) -> OutputUnit<'_, '_, '_> {
        let mut root = None;
        let mut stack: Vec<OutputUnit> = vec![];

        let mut in_ref = InRef::default();
        let mut kw_loc = KeywordLocation::default();
        for node in DfsIterator::new(self) {
            match node {
                DfsItem::Pre(e) => {
                    in_ref.pre(e);
                    kw_loc.pre(e);
                    if e.skip() {
                        continue;
                    }
                    let absolute_keyword_location = if in_ref.get() {
                        Some(e.absolute_keyword_location())
                    } else {
                        None
                    };
                    stack.push(OutputUnit {
                        valid: false,
                        keyword_location: kw_loc.get(e),
                        absolute_keyword_location,
                        instance_location: &e.instance_location,
                        error: OutputError::Leaf(&e.kind),
                    });
                }
                DfsItem::Post(e) => {
                    in_ref.post();
                    kw_loc.post();
                    if e.skip() {
                        continue;
                    }
                    let output = stack.pop().unwrap();
                    if let Some(parent) = stack.last_mut() {
                        match &mut parent.error {
                            OutputError::Leaf(_) => {
                                parent.error = OutputError::Branch(vec![output]);
                            }
                            OutputError::Branch(v) => v.push(output),
                        }
                    } else {
                        root.replace(output);
                    }
                }
            }
        }
        root.unwrap()
    }
}

impl Display for ValidationError<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut indent = Indent::default();
        let mut sloc = SchemaLocation::default();

        for node in DfsIterator::new(self) {
            match node {
                DfsItem::Pre(e) => {
                    if e.skip() {
                        continue;
                    }
                    indent.pre(f)?;
                    if f.alternate() {
                        sloc.pre(e);
                    }
                    if let ErrorKind::Schema { .. } = &e.kind {
                        write!(f, "jsonschema {}", e.kind)?;
                    } else {
                        write!(f, "at {}", quote(&e.instance_location.to_string()))?;
                        if f.alternate() {
                            write!(f, " [{}]", sloc)?;
                        }
                        write!(f, ": {}", e.kind)?;
                    }
                }
                DfsItem::Post(e) => {
                    if e.skip() {
                        continue;
                    }
                    indent.post();
                    sloc.post();
                }
            }
        }
        Ok(())
    }
}

struct DfsIterator<'a, 'v, 's> {
    root: Option<&'a ValidationError<'v, 's>>,
    stack: Vec<Frame<'a, 'v, 's>>,
}

impl<'a, 'v, 's> DfsIterator<'a, 'v, 's> {
    const fn new(err: &'a ValidationError<'v, 's>) -> Self {
        DfsIterator {
            root: Some(err),
            stack: vec![],
        }
    }
}

impl<'a, 'v, 's> Iterator for DfsIterator<'a, 'v, 's> {
    type Item = DfsItem<&'a ValidationError<'v, 's>>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(mut frame) = self.stack.pop() else {
            {
                let err = self.root.take()?;
                self.stack.push(Frame::from(err));
                return Some(DfsItem::Pre(err));
            }
        };

        if frame.causes.is_empty() {
            return Some(DfsItem::Post(frame.err));
        }

        let err = &frame.causes[0];
        frame.causes = &frame.causes[1..];
        self.stack.push(frame);
        self.stack.push(Frame::from(err));
        Some(DfsItem::Pre(err))
    }
}

struct Frame<'a, 'v, 's> {
    err: &'a ValidationError<'v, 's>,
    causes: &'a [ValidationError<'v, 's>],
}

impl<'a, 'v, 's> Frame<'a, 'v, 's> {
    fn from(err: &'a ValidationError<'v, 's>) -> Self {
        Self {
            err,
            causes: &err.causes,
        }
    }
}

enum DfsItem<T> {
    Pre(T),
    Post(T),
}

#[derive(Default)]
struct Indent {
    n: usize,
}

impl Indent {
    fn pre(&mut self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.n > 0 {
            writeln!(f)?;
            for _ in 0..self.n - 1 {
                write!(f, "  ")?;
            }
            write!(f, "- ")?;
        }
        self.n += 1;
        Ok(())
    }

    const fn post(&mut self) {
        self.n -= 1;
    }
}

#[derive(Default)]
struct SchemaLocation<'a, 's, 'v> {
    stack: Vec<&'a ValidationError<'s, 'v>>,
}

impl<'a, 's, 'v> SchemaLocation<'a, 's, 'v> {
    fn pre(&mut self, e: &'a ValidationError<'s, 'v>) {
        self.stack.push(e);
    }

    fn post(&mut self) {
        self.stack.pop();
    }
}

impl Display for SchemaLocation<'_, '_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut iter = self.stack.iter().cloned();
        let cur = iter.next_back().unwrap();
        let cur: Cow<'_, str> = match &cur.kind {
            ErrorKind::Schema { url } => Cow::Borrowed(url),
            ErrorKind::Reference { url, .. } => Cow::Borrowed(url),
            _ => Cow::Owned(cur.absolute_keyword_location().to_string()),
        };

        let Some(prev) = iter.next_back() else {
            return write!(f, "{cur}");
        };

        let p = match &prev.kind {
            ErrorKind::Schema { url } => {
                let (p, _) = split(url);
                p
            }
            ErrorKind::Reference { url, .. } => {
                let (p, _) = split(url);
                p
            }
            _ => {
                let (p, _) = split(prev.schema_url);
                p
            }
        };
        let (c, frag) = split(cur.as_ref());
        if c == p {
            write!(f, "S#{frag}")
        } else {
            write!(f, "{cur}")
        }
    }
}

#[derive(Default)]
struct KeywordLocation<'a> {
    loc: String,
    stack: Vec<(&'a str, usize)>,
}

impl<'a> KeywordLocation<'a> {
    fn pre(&mut self, e: &'a ValidationError) {
        let cur = match &e.kind {
            ErrorKind::Schema { url } => url,
            ErrorKind::Reference { url, .. } => url,
            _ => e.schema_url,
        };

        if let Some((prev, _)) = self.stack.last() {
            self.loc.push_str(&e.schema_url[prev.len()..]);
            if let ErrorKind::Reference { kw, .. } = &e.kind {
                self.loc.push('/');
                self.loc.push_str(kw);
            }
        }
        self.stack.push((cur, self.loc.len()));
    }

    fn post(&mut self) {
        self.stack.pop();
        if let Some((_, len)) = self.stack.last() {
            self.loc.truncate(*len);
        }
    }

    fn get(&mut self, cur: &'a ValidationError) -> String {
        if let ErrorKind::Reference { .. } = &cur.kind {
            self.loc.clone()
        } else if let Some(kw_path) = &cur.kind.keyword_path() {
            let len = self.loc.len();
            self.loc.push('/');
            write!(self.loc, "{}", kw_path).expect("write kw_path to String should not fail");
            let loc = self.loc.clone();
            self.loc.truncate(len);
            loc
        } else {
            self.loc.clone()
        }
    }
}

#[derive(Default)]
struct InRef {
    stack: Vec<bool>,
}

impl InRef {
    fn pre(&mut self, e: &ValidationError) {
        let in_ref: bool = self.get() || matches!(e.kind, ErrorKind::Reference { .. });
        self.stack.push(in_ref);
    }

    fn post(&mut self) {
        self.stack.pop();
    }

    fn get(&self) -> bool {
        self.stack.last().cloned().unwrap_or_default()
    }
}

pub struct FlagOutput {
    pub valid: bool,
}

impl Serialize for FlagOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("valid", &self.valid)?;
        map.end()
    }
}

impl Display for FlagOutput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write_json_to_fmt(f, self)
    }
}

pub struct OutputUnit<'e, 's, 'v> {
    pub valid: bool,
    pub keyword_location: String,

    pub absolute_keyword_location: Option<AbsoluteKeywordLocation<'s>>,

    pub instance_location: &'e InstanceLocation<'v>,
    pub error: OutputError<'e, 's, 'v>,
}

impl Serialize for OutputUnit<'_, '_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let n = 4 + self.absolute_keyword_location.as_ref().map_or(0, |_| 1);
        let mut map = serializer.serialize_map(Some(n))?;
        map.serialize_entry("valid", &self.valid)?;
        map.serialize_entry("keywordLocation", &self.keyword_location.to_string())?;
        if let Some(s) = &self.absolute_keyword_location {
            map.serialize_entry("absoluteKeywordLocation", &s.to_string())?;
        }
        map.serialize_entry("instanceLocation", &self.instance_location.to_string())?;
        let pname = match self.error {
            OutputError::Leaf(_) => "error",
            OutputError::Branch(_) => "errors",
        };
        map.serialize_entry(pname, &self.error)?;
        map.end()
    }
}

impl Display for OutputUnit<'_, '_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write_json_to_fmt(f, self)
    }
}

pub enum OutputError<'e, 's, 'v> {
    Leaf(&'e ErrorKind<'s, 'v>),

    Branch(Vec<OutputUnit<'e, 's, 'v>>),
}

impl Serialize for OutputError<'_, '_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            OutputError::Leaf(kind) => serializer.serialize_str(&kind.to_string()),
            OutputError::Branch(units) => {
                let mut seq = serializer.serialize_seq(Some(units.len()))?;
                for unit in units {
                    seq.serialize_element(unit)?;
                }
                seq.end()
            }
        }
    }
}

impl<'s> ErrorKind<'s, '_> {
    pub const fn keyword_path(&self) -> Option<KeywordPath<'s>> {
        #[inline(always)]
        const fn kw(kw: &'static str) -> Option<KeywordPath<'static>> {
            Some(KeywordPath {
                keyword: kw,
                token: None,
            })
        }

        #[inline(always)]
        const fn kw_prop<'s>(kw: &'static str, prop: &'s str) -> Option<KeywordPath<'s>> {
            Some(KeywordPath {
                keyword: kw,
                token: Some(SchemaToken::Prop(prop)),
            })
        }

        use ErrorKind::*;
        match self {
            Group => None,
            Schema { .. } => None,
            ContentSchema => kw("contentSchema"),
            PropertyName { .. } => kw("propertyNames"),
            Reference { kw: kword, .. } => kw(kword),
            RefCycle { .. } => None,
            FalseSchema => None,
            Type { .. } => kw("type"),
            Enum { .. } => kw("enum"),
            Const { .. } => kw("const"),
            Format { .. } => kw("format"),
            MinProperties { .. } => kw("minProperties"),
            MaxProperties { .. } => kw("maxProperties"),
            AdditionalProperties { .. } => kw("additionalProperty"),
            Required { .. } => kw("required"),
            Dependency { prop, .. } => kw_prop("dependencies", prop),
            DependentRequired { prop, .. } => kw_prop("dependentRequired", prop),
            MinItems { .. } => kw("minItems"),
            MaxItems { .. } => kw("maxItems"),
            Contains => kw("contains"),
            MinContains { .. } => kw("minContains"),
            MaxContains { .. } => kw("maxContains"),
            UniqueItems { .. } => kw("uniqueItems"),
            AdditionalItems { .. } => kw("additionalItems"),
            MinLength { .. } => kw("minLength"),
            MaxLength { .. } => kw("maxLength"),
            Pattern { .. } => kw("pattern"),
            ContentEncoding { .. } => kw("contentEncoding"),
            ContentMediaType { .. } => kw("contentMediaType"),
            Minimum { .. } => kw("minimum"),
            Maximum { .. } => kw("maximum"),
            ExclusiveMinimum { .. } => kw("exclusiveMinimum"),
            ExclusiveMaximum { .. } => kw("exclusiveMaximum"),
            MultipleOf { .. } => kw("multipleOf"),
            Not => kw("not"),
            AllOf => kw("allOf"),
            AnyOf => kw("anyOf"),
            OneOf(_) => kw("oneOf"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AbsoluteKeywordLocation<'s> {
    pub schema_url: &'s str,

    pub keyword_path: Option<KeywordPath<'s>>,
}

impl Display for AbsoluteKeywordLocation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.schema_url.fmt(f)?;
        if let Some(path) = &self.keyword_path {
            f.write_str("/")?;
            path.keyword.fmt(f)?;
            if let Some(token) = &path.token {
                f.write_str("/")?;
                match token {
                    SchemaToken::Prop(p) => write!(f, "{}", escape(p))?,
                    SchemaToken::Item(i) => write!(f, "{i}")?,
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]

pub struct KeywordPath<'s> {
    pub keyword: &'static str,

    pub token: Option<SchemaToken<'s>>,
}

impl Display for KeywordPath<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.keyword.fmt(f)?;
        if let Some(token) = &self.token {
            f.write_str("/")?;
            token.fmt(f)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum SchemaToken<'s> {
    Prop(&'s str),

    Item(usize),
}

impl Display for SchemaToken<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaToken::Prop(p) => write!(f, "{}", escape(p)),
            SchemaToken::Item(i) => write!(f, "{i}"),
        }
    }
}

fn write_json_to_fmt<T>(f: &mut std::fmt::Formatter, value: &T) -> Result<(), std::fmt::Error>
where
    T: ?Sized + Serialize,
{
    let s = if f.alternate() {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    };
    let s = s.map_err(|_| std::fmt::Error)?;
    f.write_str(&s)
}
