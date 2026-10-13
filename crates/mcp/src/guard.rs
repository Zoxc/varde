//! The checks every request passes before the server reads its body, in
//! this order: `Host`, `Origin`, the path, the method, then the bearer
//! token. Host and Origin first, against DNS rebinding: a page elsewhere
//! that got its name pointed at `127.0.0.1` sends its own name in both.
//! The path and `OPTIONS` before the token, so `/.well-known/oauth-*`
//! gets 404 rather than 401 and a client doesn't go looking for OAuth;
//! that tells no one anything but that `/mcp` is here. The other methods
//! (GET 405, DELETE, POST) are the server's, after the token.
//!
//! The head is read and parsed first, so one that isn't HTTP, is too
//! large, or declares a body without or past its bound (400, 431, 411,
//! 413) is refused before the guard; the body, never read before it.
//!
//! Everything is matched as text: `localhost` is never resolved, and a
//! port has to be the bound one, written as digits alone. `Host`,
//! `Origin` and `Authorization` sent more than once are refused.

use std::fmt;
use std::hint::black_box;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::http::{Request, Response, Status};
use crate::session::method_not_allowed;

/// The one path the server answers.
pub const PATH: &str = "/mcp";

/// The token a client sends as `Authorization: Bearer <token>`: 32
/// random bytes, shown as base64url without padding (43 characters).
/// [`fmt::Debug`] doesn't show it, so it stays out of logs.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Token([u8; Token::BYTES]);

impl Token {
    /// How many random bytes.
    pub const BYTES: usize = 32;
    /// The length of its text: 32 bytes in base64url, unpadded.
    pub const TEXT: usize = 43;

    /// A new token from the system's (or the browser's) random source.
    pub fn generate() -> Result<Self, getrandom::Error> {
        crate::random().map(Self)
    }

    /// The token with these bytes.
    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        Self(bytes)
    }

    /// Whether `text` is this token's text, in time that doesn't depend
    /// on where their bytes differ. Only the text not being a token's
    /// ends it early, which tells nothing of this one.
    fn matches(&self, text: &str) -> bool {
        let Ok(given) = text.parse::<Token>() else {
            return false;
        };
        let mut differs = 0u8;
        for (a, b) in given.0.iter().zip(&self.0) {
            differs |= black_box(a ^ b);
        }
        black_box(differs) == 0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

/// Text that isn't a token: not 43 base64url characters, or not the
/// canonical form of 32 bytes (the last character's spare bits set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadToken;

impl FromStr for Token {
    type Err = BadToken;

    /// The engine refuses padding and set spare bits, so only the
    /// canonical text of each token is read.
    fn from_str(text: &str) -> Result<Self, BadToken> {
        if text.len() != Self::TEXT {
            return Err(BadToken);
        }
        let bytes = URL_SAFE_NO_PAD.decode(text).map_err(|_| BadToken)?;
        Ok(Self(bytes.try_into().map_err(|_| BadToken)?))
    }
}

/// What the guard checks a request against.
#[derive(Clone, Debug)]
pub struct Guard {
    /// The port the server is bound to.
    pub port: u16,
    pub token: Token,
}

/// Why a request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No `Host`: 400, as HTTP/1.1 has it for a request without one.
    NoHost,
    /// `Host` isn't a loopback name with the bound port: 403.
    Host,
    /// `Origin` isn't loopback over `http` with the bound port, or is
    /// `null`: 403.
    Origin,
    /// A path other than [`PATH`]: 404.
    Path,
    /// `OPTIONS`: 405. No CORS, so no preflight to answer.
    Options,
    /// No `Authorization`, or not the token: 401.
    Token,
}

impl Refusal {
    /// The response to send.
    pub fn response(self) -> Response {
        match self {
            Refusal::NoHost => Response::empty(Status::BadRequest),
            Refusal::Host | Refusal::Origin => Response::empty(Status::Forbidden),
            Refusal::Path => Response::empty(Status::NotFound),
            Refusal::Options => method_not_allowed(),
            Refusal::Token => {
                Response::empty(Status::Unauthorized).with_header("WWW-Authenticate", "Bearer")
            }
        }
    }
}

/// The longest `Authorization` value looked at: `Bearer `, the token,
/// and room for spaces. Anything longer is refused unread.
const AUTHORIZATION: usize = 64;

impl Guard {
    /// Checks `request`, in the order the module docs give.
    pub fn check(&self, request: &Request) -> Result<(), Refusal> {
        let host = only(request, "host", Refusal::Host)?.ok_or(Refusal::NoHost)?;
        if !self.is_host(host) {
            return Err(Refusal::Host);
        }
        if let Some(origin) = only(request, "origin", Refusal::Origin)?
            && !self.is_origin(origin)
        {
            return Err(Refusal::Origin);
        }
        if request.path != PATH {
            return Err(Refusal::Path);
        }
        if request.method == "OPTIONS" {
            return Err(Refusal::Options);
        }
        let authorization =
            only(request, "authorization", Refusal::Token)?.ok_or(Refusal::Token)?;
        if !self.is_authorization(authorization) {
            return Err(Refusal::Token);
        }
        Ok(())
    }

    /// `127.0.0.1`, `localhost` (in any case, as names are) or `[::1]`,
    /// then `:` and the port.
    fn is_host(&self, host: &str) -> bool {
        let Some((name, port)) = host.trim_matches([' ', '\t']).rsplit_once(':') else {
            return false;
        };
        let loopback =
            name == "127.0.0.1" || name == "[::1]" || name.eq_ignore_ascii_case("localhost");
        loopback && self.is_port(port)
    }

    /// `http://` and a loopback host with the port, nothing after. Not
    /// `https`, which the server doesn't speak, and not another port,
    /// which is another program.
    fn is_origin(&self, origin: &str) -> bool {
        let origin = origin.trim_matches([' ', '\t']);
        let Some(scheme) = origin.get(..7) else {
            return false;
        };
        scheme.eq_ignore_ascii_case("http://") && self.is_host(&origin[7..])
    }

    /// The bound port, digits alone with no leading zero.
    fn is_port(&self, port: &str) -> bool {
        !port.is_empty()
            && port.len() <= 5
            && port.bytes().all(|b| b.is_ascii_digit())
            && !(port.len() > 1 && port.starts_with('0'))
            && port.parse::<u16>() == Ok(self.port)
    }

    /// `Bearer` in any case, spaces, then the token.
    fn is_authorization(&self, value: &str) -> bool {
        if value.len() > AUTHORIZATION {
            return false;
        }
        let Some((scheme, token)) = value.trim_matches([' ', '\t']).split_once(' ') else {
            return false;
        };
        scheme.eq_ignore_ascii_case("bearer") && self.token.matches(token.trim_start_matches(' '))
    }
}

/// The header named `name` if sent once, `refusal` if sent more often.
fn only<'a>(
    request: &'a Request,
    name: &str,
    refusal: Refusal,
) -> Result<Option<&'a str>, Refusal> {
    request.once(name).map_err(|_| refusal)
}

#[cfg(test)]
mod tests;
