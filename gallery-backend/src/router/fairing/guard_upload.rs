use rocket::Request;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};

use crate::public::error::{AppError, ErrorKind};
use crate::router::GuardError;

use super::VALIDATION;
use super::auth_utils::{try_authorize_upload_via_share, try_jwt_cookie_auth};

pub struct GuardUpload {
    pub is_owner: bool,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardUpload {
    type Error = GuardError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match try_jwt_cookie_auth(req, &VALIDATION) {
            Ok(_) => Outcome::Success(GuardUpload { is_owner: true }),
            Err(err) => {
                if !super::guard_gallery::public_gallery_enabled()
                    && try_authorize_upload_via_share(req)
                {
                    return Outcome::Success(GuardUpload { is_owner: false });
                }
                let full_err =
                    AppError::from_err(ErrorKind::Auth, err).context("Authentication error");
                Outcome::Error((Status::Unauthorized, full_err))
            }
        }
    }
}
