use std::borrow::Cow;

pub(crate) struct Regex {
    source: String,
    compiled: regress::Regex,
}

impl Regex {
    pub(crate) fn new(pattern: &str) -> Result<Self, regress::Error> {
        Ok(Self {
            source: pattern.to_owned(),
            compiled: regress::Regex::with_flags(
                pattern,
                regress::Flags {
                    unicode: true,
                    ..Default::default()
                },
            )?,
        })
    }

    pub(crate) fn is_match(&self, text: &str) -> bool {
        self.compiled.find(text).is_some()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.source
    }
}

pub(crate) fn convert(pattern: &str) -> Result<Cow<'_, str>, Box<dyn std::error::Error>> {
    Regex::new(pattern)?;
    Ok(Cow::Borrowed(pattern))
}
