use crate::otp::Algorithm;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub struct OtpParams {
    pub issuer: String,
    pub label: String,
    pub secret: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: Algorithm,
}

pub fn decode_qr_from_path<P: AsRef<Path>>(path: P) -> Result<OtpParams> {
    #[allow(deprecated)]
    let image = image::open(path.as_ref()).context("Unable to read the image")?;
    #[allow(deprecated)]
    let gray = image.to_luma();
    let mut prepared = rqrr::PreparedImage::prepare(gray);
    let grids = prepared.detect_grids();
    let grid = grids
        .into_iter()
        .next()
        .context("No QR code detected in the image")?;
    let (_meta, payload) = grid.decode().context("Unable to read QR code contents")?;
    parse_otpauth(&payload)
}

pub fn parse_otpauth(uri: &str) -> Result<OtpParams> {
    let url = url::Url::parse(uri).context("Invalid otpauth URI")?;
    if url.scheme() != "otpauth" {
        bail!("Unsupported URI scheme: expected otpauth");
    }
    let host = url
        .host_str()
        .context("Missing OTP type in the otpauth URI")?;
    if !host.eq_ignore_ascii_case("totp") {
        bail!("Only TOTP entries can be imported");
    }
    let path = url.path().trim_start_matches('/');
    let (label, issuer_from_label) = match path.split_once(':') {
        Some((label_part, account)) => (decode_label(account), decode_label(label_part)),
        None => (decode_label(path), String::new()),
    };
    let mut params = url.query_pairs();
    let secret = params
        .find(|(name, _)| name == "secret")
        .map(|(_, value)| value.into_owned())
        .context("Missing secret parameter")?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        bail!("Empty secret parameter");
    }
    let issuer = params
        .find(|(name, _)| name == "issuer")
        .map(|(_, value)| value.into_owned())
        .unwrap_or(issuer_from_label);
    let digits = params
        .find(|(name, _)| name == "digits")
        .and_then(|(_, value)| value.parse::<u8>().ok())
        .unwrap_or(6);
    let period = params
        .find(|(name, _)| name == "period")
        .and_then(|(_, value)| value.parse::<u32>().ok())
        .unwrap_or(30);
    let algorithm = params
        .find(|(name, _)| name == "algorithm")
        .map(|(_, value)| value.to_ascii_uppercase())
        .map(|value| match value.as_str() {
            "SHA256" => Algorithm::Sha256,
            "SHA512" => Algorithm::Sha512,
            _ => Algorithm::Sha1,
        })
        .unwrap_or(Algorithm::Sha1);

    Ok(OtpParams {
        issuer: if issuer.is_empty() {
            "Imported".to_string()
        } else {
            issuer
        },
        label: if label.is_empty() {
            "Entry".to_string()
        } else {
            label
        },
        secret,
        digits,
        period,
        algorithm,
    })
}

fn decode_label(encoded: &str) -> String {
    percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| encoded.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_otpauth_uri() {
        let uri = "otpauth://totp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Example&algorithm=SHA1&digits=6&period=30";
        let params = parse_otpauth(uri).unwrap();
        assert_eq!(params.issuer, "Example");
        assert_eq!(params.label, "alice@example.com");
        assert_eq!(params.secret, "JBSWY3DPEHPK3PXP");
        assert_eq!(params.digits, 6);
        assert_eq!(params.period, 30);
        assert_eq!(params.algorithm, Algorithm::Sha1);
    }

    #[test]
    fn rejects_non_totp() {
        let uri = "otpauth://hotp/Example:alice?secret=JBSWY3DPEHPK3PXP&counter=0";
        assert!(parse_otpauth(uri).is_err());
    }
}
