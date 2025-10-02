mod error;
mod jwks;
mod jwt;
mod layer;

use context::Authentication;
use http::request::Parts;
pub(crate) use layer::*;

type AuthResult<T> = Result<T, error::AuthError>;

pub(crate) trait NativeProviderAuthentication {
    fn authenticate(&self, parts: &Parts) -> Authentication;
}

impl NativeProviderAuthentication for () {
    fn authenticate(&self, _parts: &Parts) -> Authentication {
        Default::default()
    }
}

impl<F> NativeProviderAuthentication for F
where
    F: Fn(&Parts) -> Authentication + Send + Sync,
{
    fn authenticate(&self, parts: &Parts) -> Authentication {
        (self)(parts)
    }
}
