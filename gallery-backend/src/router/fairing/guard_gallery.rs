use rocket::Request;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};

use super::{VALIDATION, auth_utils::try_jwt_cookie_auth};
use crate::public::error::{AppError, ErrorKind};
use crate::router::{GuardError, claims::Claims};

pub fn public_gallery_enabled() -> bool {
    std::env::var("UROCISSA_PUBLIC_GALLERY").as_deref() == Ok("1")
}

/// Gallery reads only. This guard must never authorize a mutation.
pub struct GuardGallery {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardGallery {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match try_jwt_cookie_auth(req, &VALIDATION) {
            Ok(claims) => Outcome::Success(Self { claims }),
            Err(_) if public_gallery_enabled() => Outcome::Success(Self {
                claims: Claims::new_public(),
            }),
            Err(err) => Outcome::Error((
                Status::Unauthorized,
                AppError::from_err(ErrorKind::Auth, err).context("Authentication error"),
            )),
        }
    }
}
