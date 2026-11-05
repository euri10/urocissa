//! Photo audience is independent of album membership. Absence always means private.
use arrayvec::ArrayString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    #[default]
    Private,
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Viewer {
    Owner,
    Anonymous,
    Share(ArrayString<64>),
}

impl Viewer {
    /// A share is an explicit grant only for members of that share's album.
    pub fn can_view(self, audience: Audience, trashed: bool, share_member: bool) -> bool {
        match self {
            Self::Owner => true,
            Self::Anonymous => !trashed && audience == Audience::Public,
            Self::Share(_) => !trashed && share_member,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_matrix() {
        let share = Viewer::Share(ArrayString::from("album").unwrap());
        for audience in [None, Some(Audience::Private), Some(Audience::Public)] {
            for trashed in [false, true] {
                for member in [false, true] {
                    for (viewer, expected) in [
                        (Viewer::Owner, true),
                        (
                            Viewer::Anonymous,
                            !trashed && audience == Some(Audience::Public),
                        ),
                        (share, !trashed && member),
                    ] {
                        assert_eq!(
                            viewer.can_view(audience.unwrap_or_default(), trashed, member),
                            expected,
                            "{viewer:?}, {audience:?}, trashed={trashed}, member={member}"
                        );
                    }
                }
            }
        }
    }
}
