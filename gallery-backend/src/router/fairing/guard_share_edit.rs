use rocket::Request;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};

use super::guard_auth::GuardAuth;
use super::guard_gallery::public_gallery_enabled;
use super::guard_share::GuardShare;
use crate::router::claims::{Claims, Role};
use crate::router::{AppError, ErrorKind, GuardError};

/// Capability for the two historically share-editable metadata routes only.
/// Handlers must additionally enforce the share's resource boundary.
pub fn can_edit_share_resources(claims: &Claims) -> bool {
    claims.is_admin() || (!public_gallery_enabled() && matches!(claims.role, Role::Share(_)))
}

pub struct GuardShareEdit {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardShareEdit {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        if public_gallery_enabled() {
            return GuardAuth::from_request(req).await.map(|_| Self {
                claims: Claims::new_admin(),
            });
        }

        match GuardShare::from_request(req).await {
            Outcome::Success(guard) if can_edit_share_resources(&guard.claims) => {
                Outcome::Success(Self {
                    claims: guard.claims,
                })
            }
            Outcome::Success(_) => Outcome::Error((
                Status::Forbidden,
                AppError::new(ErrorKind::PermissionDenied, "Share editing is not allowed"),
            )),
            Outcome::Error(error) => Outcome::Error(error),
            Outcome::Forward(status) => Outcome::Forward(status),
        }
    }
}
