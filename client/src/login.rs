//! The host's login packet.
//!
//! The embedded server runs offline while LAN is closed, so it reads the
//! login tokens without checking any signature (recon §6). The host therefore
//! sends *unsecured* JWTs (`"alg": "none"`, empty signature) that carry just
//! what the server needs. No key pair, no Xbox sign-in.

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use pumpkin_auth::{
    jwt::extract_oidc_token_player_claims,
    p384::{AffinePoint, elliptic_curve::sec1::ToSec1Point},
};
use pumpkin_protocol::bedrock::server::login::SLogin;
use pumpkin_world::CURRENT_BEDROCK_MC_PROTOCOL;
use serde_json::{Value, json};
use uuid::Uuid;

/// The login `AuthenticationType` the official client sends (0, "Full").
/// Offline, the server only checks that it's a known value.
const AUTHENTICATION_TYPE: u8 = 0;

/// Builds the host's `SLogin` for display name `name` and this install's Xbox
/// user id `xid` (`identity.rs`).
pub fn host_login(name: &str, xid: &str) -> SLogin {
    let jwt = json!({ "AuthenticationType": AUTHENTICATION_TYPE, "Token": host_token(name, xid) });

    SLogin {
        protocol_version: CURRENT_BEDROCK_MC_PROTOCOL as i32,
        jwt: jwt.to_string().into_bytes(),
        raw_token: unsecured_jwt(&client_data()).into_bytes(),
    }
}

/// The identity token in the host's login: the claims the server reads the
/// player's name and `xid` from, and derives their UUID from.
pub fn host_token(name: &str, xid: &str) -> String {
    unsecured_jwt(&json!({
        "xname": name,
        "xid": xid,
        "cpk": placeholder_cpk(),
    }))
}

/// The UUID the server gives the host, read the way the server reads it:
/// pumpkin-auth's own parser, over the token the host sends. So the two can't
/// drift apart. It depends on the `xid` alone.
///
/// # Errors
/// If the parser can't read the token or its UUID, which would stop the
/// server's login too.
pub fn host_uuid(name: &str, xid: &str) -> Result<Uuid, String> {
    let claims = extract_oidc_token_player_claims(&host_token(name, xid))
        .map_err(|err| format!("the server can't read the host's token: {err}"))?;
    Uuid::parse_str(&claims.uuid).map_err(|err| format!("the host's UUID `{}` isn't one: {err}", claims.uuid))
}

/// The `ClientData` claims: the 10 fields the server requires.
///
/// Device and version values are the ones the official Bedrock 1.26.52 client
/// sent on Windows (M2 Step 1 capture); the identifiers are fixed dev values.
fn client_data() -> Value {
    json!({
        "ClientRandomId": 1,
        "DeviceId": "limonite-dev",
        "DeviceOS": 8,
        "GameVersion": "1.26.52",
        "LanguageCode": "en_US",
        "CurrentInputMode": 1,
        "DefaultInputMode": 1,
        "UIProfile": 0,
        "ServerAddress": "limonite-local",
        "MaxViewDistance": 96,
    })
}

/// A JWT with `"alg": "none"` and an empty signature: `header.payload.`.
fn unsecured_jwt(claims: &Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "none" }).to_string());
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
    format!("{header}.{payload}.")
}

/// The `cpk` (client public key) claim the server insists on.
///
/// It must parse as a P-384 point, but nothing ever verifies a signature with
/// it, so any valid point works. The curve's generator point makes it plainly
/// a placeholder. Encoded as uncompressed SEC1 (97 bytes) in standard base64.
fn placeholder_cpk() -> String {
    STANDARD.encode(AffinePoint::GENERATOR.to_sec1_point(false).as_bytes())
}

#[cfg(test)]
mod tests {
    use pumpkin_auth::jwt::{decode_b64_url_nopad, extract_cpk_from_token};
    use pumpkin_protocol::bedrock::server::login::ClientData;

    use super::*;

    /// M4's fixed development identity.
    const XID: &str = "1000000000000001";

    fn token(login: &SLogin) -> String {
        let jwt: Value = serde_json::from_slice(&login.jwt).unwrap();
        assert_eq!(jwt["AuthenticationType"], 0);
        jwt["Token"].as_str().unwrap().to_string()
    }

    #[test]
    fn the_server_reads_our_name_and_xid() {
        let claims = extract_oidc_token_player_claims(&token(&host_login("Limonite", XID))).unwrap();
        assert_eq!(claims.display_name, "Limonite");
        assert_eq!(claims.xuid, XID);
    }

    #[test]
    fn the_hosts_uuid_is_the_one_the_server_gave_it() {
        // The server saved M4's dev host, which logged in with this xid, as
        // run/client/world/players/data/eb9cf2d5-8d1a-3340-b297-a4713ed878e4.dat.
        let uuid = host_uuid("Limonite", XID).unwrap();
        assert_eq!(uuid.to_string(), "eb9cf2d5-8d1a-3340-b297-a4713ed878e4");
        assert_eq!(host_uuid("Someone Else", XID).unwrap(), uuid, "the name doesn't change it");
        assert_ne!(host_uuid("Limonite", "1000000000000002").unwrap(), uuid);
    }

    #[test]
    fn the_server_accepts_our_cpk() {
        let cpk = extract_cpk_from_token(&token(&host_login("Limonite", XID))).unwrap();
        assert_eq!(cpk.as_affine(), &AffinePoint::GENERATOR);
    }

    #[test]
    fn the_server_parses_our_client_data() {
        let login = host_login("Limonite", XID);
        let raw_token = std::str::from_utf8(&login.raw_token).unwrap();
        let payload = raw_token.split('.').nth(1).unwrap();
        let client_data: ClientData =
            serde_json::from_slice(&decode_b64_url_nopad(payload).unwrap()).unwrap();
        assert_eq!(client_data.device_os, 8);
        assert_eq!(client_data.game_version, "1.26.52");
        assert_eq!(client_data.language_code, "en_US");
    }

    #[test]
    fn tokens_are_unsecured_jwts() {
        let login = host_login("Limonite", XID);
        for jwt in [token(&login), String::from_utf8(login.raw_token).unwrap()] {
            let parts: Vec<_> = jwt.split('.').collect();
            assert_eq!(parts.len(), 3);
            assert!(parts[2].is_empty(), "signature should be empty");
            let header: Value =
                serde_json::from_slice(&decode_b64_url_nopad(parts[0]).unwrap()).unwrap();
            assert_eq!(header, json!({ "alg": "none" }));
        }
    }
}
