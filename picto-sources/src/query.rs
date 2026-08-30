use crate::{SourceError, SourceErrorKind};

#[derive(Debug, Clone, Copy)]
pub struct SearchQueryPolicy {
    source_name: &'static str,
    reserved_prefixes: &'static [&'static str],
}

impl SearchQueryPolicy {
    pub const fn new(
        source_name: &'static str,
        reserved_prefixes: &'static [&'static str],
    ) -> Self {
        Self {
            source_name,
            reserved_prefixes,
        }
    }

    pub fn validate(&self, query: &str) -> Result<(), SourceError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                format!("{} search cannot be empty", self.source_name),
                false,
            ));
        }
        if query.split_whitespace().any(|token| {
            let token = token.to_ascii_lowercase();
            self.reserved_prefixes
                .iter()
                .any(|prefix| token.starts_with(prefix))
        }) {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                format!("{} search cannot control traversal", self.source_name),
                false,
            ));
        }
        Ok(())
    }
}
