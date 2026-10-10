//! The login of the Moodle mobile app: the browser opens Moodle's launch page,
//! the student signs in there, and Moodle redirects to `<scheme>://token=...`.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use uni_core::{Error, Result};

pub fn launch_url(base_url: &str, passport: &str, scheme: &str) -> String {
    format!(
        "{}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport={passport}&urlscheme={scheme}",
        base_url.trim_end_matches('/')
    )
}

/// The token from a redirect. Its payload is base64 of
/// `<signature>:::<token>[:::<private token>]` with the signature
/// `md5(<site URL><passport>)`, which ties it to one login attempt.
pub fn parse_redirect(
    redirect: &str,
    scheme: &str,
    base_url: &str,
    passport: &str,
) -> Result<String> {
    let invalid =
        |why: &str| Error::InvalidInput(format!("not a valid Moodle login redirect: {why}"));

    let payload = redirect
        .trim()
        .strip_prefix(&format!("{scheme}://token="))
        .ok_or_else(|| invalid("it does not start with the expected link prefix"))?;
    let decoded = STANDARD
        .decode(payload.trim_end_matches('/'))
        .map_err(|_| invalid("the token part is not base64"))?;
    let decoded = String::from_utf8(decoded).map_err(|_| invalid("the token part is not text"))?;

    let mut parts = decoded.split(":::");
    let signature = parts.next().unwrap_or_default();
    let token = parts.next().ok_or_else(|| invalid("no token inside"))?;
    if !signature.eq_ignore_ascii_case(&signature_for(base_url, passport)) {
        return Err(invalid(
            "it does not belong to this login attempt; start the login again",
        ));
    }
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(invalid("the token has an unexpected format"));
    }
    Ok(token.to_owned())
}

fn signature_for(base_url: &str, passport: &str) -> String {
    let site = base_url.trim_end_matches('/');
    format!("{:x}", md5::compute(format!("{site}{passport}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://moodle.example";

    fn redirect(inner: &str) -> String {
        format!("unimcp://token={}", STANDARD.encode(inner))
    }

    fn signed(rest: &str) -> String {
        redirect(&format!("{}:::{rest}", signature_for(BASE, "p1")))
    }

    fn parse(redirect: &str) -> Result<String> {
        parse_redirect(redirect, "unimcp", BASE, "p1")
    }

    #[test]
    fn extracts_the_token() {
        assert_eq!(parse(&signed("abc123:::private")).expect("token"), "abc123");
        assert_eq!(parse(&signed("abc123")).expect("token"), "abc123");
    }

    #[test]
    fn rejects_other_input() {
        assert!(parse("https://example.org").is_err());
        assert!(parse(&format!("moodlemobile://token={}", STANDARD.encode("x"))).is_err());
        assert!(parse(&redirect("no-separator")).is_err());
        assert!(parse(&signed("bad token")).is_err());
    }

    #[test]
    fn rejects_a_redirect_from_another_attempt() {
        let other = redirect(&format!("{}:::abc123", signature_for(BASE, "p2")));
        assert!(parse(&other).is_err());
    }

    #[test]
    fn signature_is_md5_of_site_and_passport() {
        // md5("abc")
        assert_eq!(
            signature_for("ab/", "c"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn builds_the_launch_url() {
        let url = launch_url("https://moodle.example/", "p1", "unimcp");
        assert!(url.starts_with("https://moodle.example/admin/tool/mobile/launch.php?"));
        assert!(url.contains("passport=p1"));
        assert!(url.ends_with("urlscheme=unimcp"));
    }
}
