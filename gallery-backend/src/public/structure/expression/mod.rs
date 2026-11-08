use super::audience::Audience;
use arrayvec::ArrayString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum FilterValue {
    Value(String),
    Exists(bool),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum AlbumFilterValue {
    Value(ArrayString<64>),
    Exists(bool),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TagSource {
    User,
    Auto,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum Expression {
    Or(Vec<Expression>),
    And(Vec<Expression>),
    Not(Box<Expression>),
    Tag(FilterValue),
    Source(TagSource),
    ExtType(String),
    Ext(String),
    Model(FilterValue),
    Make(FilterValue),
    Path(String),
    Album(AlbumFilterValue),
    Audience(Audience),
    Semantic(String),
    Similar(String),
    Any(String),
    // Boolean field filters
    Favorite(bool),
    Archived(bool),
    Trashed(bool),
}

impl Expression {
    pub fn contains_source(&self) -> bool {
        match self {
            Self::Source(_) => true,
            Self::Or(items) | Self::And(items) => items.iter().any(Self::contains_source),
            Self::Not(item) => item.contains_source(),
            _ => false,
        }
    }

    pub fn contains_tag_search(&self) -> bool {
        self.contains_tag() || self.contains_source()
    }

    pub fn contains_tag(&self) -> bool {
        match self {
            Self::Tag(_) => true,
            Self::Or(items) | Self::And(items) => items.iter().any(Self::contains_tag),
            Self::Not(item) => item.contains_tag(),
            _ => false,
        }
    }

    /// Ranked text and image queries both use the owner-only semantic search path.
    pub fn contains_semantic(&self) -> bool {
        match self {
            Self::Semantic(_) | Self::Similar(_) => true,
            Self::Or(expressions) | Self::And(expressions) => {
                expressions.iter().any(Self::contains_semantic)
            }
            Self::Not(expression) => expression.contains_semantic(),
            _ => false,
        }
    }

    pub fn contains_audience(&self) -> bool {
        match self {
            Self::Audience(_) => true,
            Self::Or(expressions) | Self::And(expressions) => {
                expressions.iter().any(Self::contains_audience)
            }
            Self::Not(expression) => expression.contains_audience(),
            _ => false,
        }
    }

    pub fn contains_path(&self) -> bool {
        match self {
            Self::Path(_) => true,
            Self::Or(expressions) | Self::And(expressions) => {
                expressions.iter().any(Self::contains_path)
            }
            Self::Not(expression) => expression.contains_path(),
            _ => false,
        }
    }
}
