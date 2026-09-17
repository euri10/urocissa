use rocket::Request;
use rocket::request::{FromRequest, Outcome};

use super::auth_utils::{try_resolve_share_from_headers, try_resolve_share_from_query};
use super::guard_gallery::GuardGallery;
use crate::router::GuardError;
use crate::router::claims::claims::Claims;

pub struct GuardShare {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardShare {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        // headers
        match try_resolve_share_from_headers(req) {
            Ok(Some(claims)) => return Outcome::Success(GuardShare { claims }),
            Ok(None) => {} // No share headers, continue
            Err(err) => {
                let status = err.http_status();
                return Outcome::Error((status, err));
            }
        }

        // query
        match try_resolve_share_from_query(req) {
            Ok(Some(claims)) => return Outcome::Success(GuardShare { claims }),
            Ok(None) => {}
            Err(err) => {
                let status = err.http_status();
                return Outcome::Error((status, err));
            }
        }

        // Fall back to JWT cookie authentication (Admin)
        GuardGallery::from_request(req)
            .await
            .map(|guard| GuardShare {
                claims: guard.claims,
            })
    }
}
