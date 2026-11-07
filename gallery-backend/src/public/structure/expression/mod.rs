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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum Expression {
    Or(Vec<Expression>),
    And(Vec<Expression>),
    Not(Box<Expression>),
    Tag(FilterValue),
    ExtType(String),
    Ext(String),
    Model(FilterValue),
    Make(FilterValue),
    Path(String),
    Album(AlbumFilterValue),
    Audience(Audience),
    Semantic(String),
    Any(String),
    // Boolean field filters
    Favorite(bool),
    Archived(bool),
    Trashed(bool),
}

impl Expression {
    pub fn contains_semantic(&self) -> bool {
        match self {
            Self::Semantic(_) => true,
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
