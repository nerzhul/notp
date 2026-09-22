use crate::otp::{encode_base32, Algorithm};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use std::path::Path;

pub struct OtpParams {
    pub issuer: String,
    pub label: String,
    pub secret: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: Algorithm,
}

pub fn decode_qr_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<OtpParams>> {
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
    decode_qr_payload(&payload).map_err(|err| {
        let preview: String = payload.chars().take(120).collect();
        let ellipsis = if payload.chars().count() > preview.chars().count() {
            "\u{2026}"
        } else {
            ""
        };
        anyhow::Error::msg(format!(
            "The QR code cannot be imported as an otpauth entry (read: {preview}{ellipsis}): {err:#}"
        ))
    })
}

pub fn decode_qr_from_bytes(bytes: &[u8]) -> Result<Vec<OtpParams>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    #[allow(deprecated)]
    let image = match image::load_from_memory(bytes) {
        Ok(image) => image,
        Err(_) => return Ok(Vec::new()),
    };
    #[allow(deprecated)]
    let gray = image.to_luma();
    let mut prepared = rqrr::PreparedImage::prepare(gray);
    let grids = prepared.detect_grids();
    let Some(grid) = grids.into_iter().next() else {
        return Ok(Vec::new());
    };
    let (_meta, payload) = match grid.decode() {
        Ok(decoded) => decoded,
        Err(_) => return Ok(Vec::new()),
    };
    decode_qr_payload(&payload)
}

pub fn decode_qr_payload(payload: &str) -> Result<Vec<OtpParams>> {
    let url = url::Url::parse(payload).context("The QR code does not contain a valid URI")?;
    match url.scheme() {
        "otpauth" => Ok(vec![parse_otpauth(url.as_str())?]),
        "otpauth-migration" => parse_migration(&url),
        other => bail!("Unsupported URI scheme: {other}"),
    }
}

pub fn parse_otpauth(uri: &str) -> Result<OtpParams> {
    let url = url::Url::parse(uri).context("Invalid otpauth URI")?;
    parse_otpauth_url(&url)
}

fn parse_otpauth_url(url: &url::Url) -> Result<OtpParams> {
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

fn parse_migration(url: &url::Url) -> Result<Vec<OtpParams>> {
    let host = url.host_str().unwrap_or("");
    if !host.eq_ignore_ascii_case("offline") {
        bail!(
            "Unsupported otpauth-migration host: expected \"offline\", got {:?}",
            host
        );
    }
    let data = url
        .query_pairs()
        .find(|(name, _)| name == "data")
        .map(|(_, value)| value.into_owned())
        .context("Missing data parameter in otpauth-migration URI")?;
    if data.is_empty() {
        bail!("The data parameter in the otpauth-migration URI is empty");
    }
    let bytes = decode_migration_data(&data).context("Unable to base64-decode the otpauth-migration payload")?;
    let payload = parse_migration_payload(&bytes)
        .context("Unable to parse the otpauth-migration protobuf payload")?;
    if payload.is_empty() {
        bail!("The otpauth-migration payload contains no entries");
    }
    let mut entries = Vec::with_capacity(payload.len());
    let mut skipped = 0_usize;
    for parameter in payload {
        match migration_parameter_to_params(parameter) {
            Ok(entry) => entries.push(entry),
            Err(MigrationSkip::UnsupportedType) => skipped += 1,
            Err(MigrationSkip::Other(error)) => return Err(error),
        }
    }
    if entries.is_empty() {
        bail!(
            "The otpauth-migration payload contains no supported TOTP entries ({} skipped)",
            skipped
        );
    }
    Ok(entries)
}

enum MigrationSkip {
    UnsupportedType,
    Other(anyhow::Error),
}

fn migration_parameter_to_params(parameter: MigrationOtpParameter) -> Result<OtpParams, MigrationSkip> {
    if parameter.otp_type != 2 {
        return Err(MigrationSkip::UnsupportedType);
    }
    if parameter.secret.is_empty() {
        return Err(MigrationSkip::Other(anyhow!(
            "An otpauth-migration entry is missing its secret"
        )));
    }
    let (issuer, label) = split_migration_name(&parameter.issuer, &parameter.name);
    let algorithm = match parameter.algorithm {
        2 => Algorithm::Sha256,
        3 => Algorithm::Sha512,
        _ => Algorithm::Sha1,
    };
    let digits = match parameter.digits {
        2 => 8_u8,
        _ => 6_u8,
    };
    Ok(OtpParams {
        issuer,
        label,
        secret: encode_base32(&parameter.secret),
        digits,
        period: 30,
        algorithm,
    })
}

fn split_migration_name(issuer: &str, name: &str) -> (String, String) {
    if let Some((label_issuer, account)) = name.split_once(':') {
        let resolved_issuer = if issuer.is_empty() {
            label_issuer.to_string()
        } else {
            issuer.to_string()
        };
        return (resolved_issuer, account.trim().to_string());
    }
    let resolved_issuer = if issuer.is_empty() {
        "Imported".to_string()
    } else {
        issuer.to_string()
    };
    (resolved_issuer, name.trim().to_string())
}

fn decode_label(encoded: &str) -> String {
    percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| encoded.to_string())
}

fn decode_migration_data(data: &str) -> Result<Vec<u8>> {
    let trimmed = data.trim().trim_end_matches('=');
    let candidates: [base64::engine::GeneralPurpose; 4] = [
        URL_SAFE_NO_PAD,
        URL_SAFE,
        STANDARD_NO_PAD,
        STANDARD,
    ];
    let mut last_err: Option<base64::DecodeError> = None;
    for engine in &candidates {
        match engine.decode(trimmed.as_bytes()) {
            Ok(bytes) => return Ok(bytes),
            Err(err) => last_err = Some(err),
        }
    }
    match last_err {
        Some(err) => Err(anyhow!(err)),
        None => bail!("Empty base64 data"),
    }
}

struct MigrationOtpParameter {
    secret: Vec<u8>,
    name: String,
    issuer: String,
    algorithm: i32,
    otp_type: i32,
    digits: i32,
}

fn parse_migration_payload(data: &[u8]) -> Result<Vec<MigrationOtpParameter>> {
    let mut pos = 0;
    let mut parameters = Vec::new();
    while pos < data.len() {
        let tag = read_varint(data, &mut pos).context("Truncated migration payload")?;
        let field = tag >> 3;
        let wire_type = tag & 7;
        if field == 1 && wire_type == 2 {
            let length = read_varint(data, &mut pos).context("Truncated migration payload")? as usize;
            if pos + length > data.len() {
                bail!("Truncated migration payload");
            }
            let bytes = &data[pos..pos + length];
            pos += length;
            parameters.push(parse_migration_parameter(bytes)?);
        } else {
            skip_field(data, &mut pos, wire_type)?;
        }
    }
    Ok(parameters)
}

fn parse_migration_parameter(data: &[u8]) -> Result<MigrationOtpParameter> {
    let mut pos = 0;
    let mut secret = Vec::new();
    let mut name = String::new();
    let mut issuer = String::new();
    let mut algorithm: i32 = 1;
    let mut otp_type: i32 = 2;
    let mut digits: i32 = 6;
    while pos < data.len() {
        let tag = read_varint(data, &mut pos).context("Truncated migration entry")?;
        let field = tag >> 3;
        let wire_type = tag & 7;
        match (field, wire_type) {
            (1, 2) => {
                let length =
                    read_varint(data, &mut pos).context("Truncated migration entry")? as usize;
                if pos + length > data.len() {
                    bail!("Truncated migration entry");
                }
                secret = data[pos..pos + length].to_vec();
                pos += length;
            }
            (2, 2) => {
                let length =
                    read_varint(data, &mut pos).context("Truncated migration entry")? as usize;
                if pos + length > data.len() {
                    bail!("Truncated migration entry");
                }
                name = String::from_utf8_lossy(&data[pos..pos + length]).into_owned();
                pos += length;
            }
            (3, 2) => {
                let length =
                    read_varint(data, &mut pos).context("Truncated migration entry")? as usize;
                if pos + length > data.len() {
                    bail!("Truncated migration entry");
                }
                issuer = String::from_utf8_lossy(&data[pos..pos + length]).into_owned();
                pos += length;
            }
            (4, 0) => algorithm = read_varint(data, &mut pos)? as i32,
            (5, 0) => digits = read_varint(data, &mut pos)? as i32,
            (6, 0) => otp_type = read_varint(data, &mut pos)? as i32,
            (_, 0) => {
                read_varint(data, &mut pos).context("Truncated migration entry")?;
            }
            (_, 2) => {
                let length =
                    read_varint(data, &mut pos).context("Truncated migration entry")? as usize;
                if pos + length > data.len() {
                    bail!("Truncated migration entry");
                }
                pos += length;
            }
            (_, 5) => {
                if pos + 4 > data.len() {
                    bail!("Truncated migration entry");
                }
                pos += 4;
            }
            (_, 1) => {
                if pos + 8 > data.len() {
                    bail!("Truncated migration entry");
                }
                pos += 8;
            }
            _ => bail!("Unsupported protobuf wire type in migration entry"),
        }
    }
    Ok(MigrationOtpParameter {
        secret,
        name,
        issuer,
        algorithm,
        otp_type,
        digits,
    })
}

fn read_varint(data: &[u8], pos: &mut usize) -> Result<u64> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        if *pos >= data.len() {
            bail!("Truncated varint");
        }
        if shift >= 64 {
            bail!("Varint overflow");
        }
        let byte = data[*pos];
        *pos += 1;
        result |= (u64::from(byte) & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
}

fn skip_field(data: &[u8], pos: &mut usize, wire_type: u64) -> Result<()> {
    match wire_type {
        0 => {
            read_varint(data, pos)?;
        }
        1 => {
            if *pos + 8 > data.len() {
                bail!("Truncated payload");
            }
            *pos += 8;
        }
        2 => {
            let length = read_varint(data, pos)? as usize;
            if *pos + length > data.len() {
                bail!("Truncated payload");
            }
            *pos += length;
        }
        5 => {
            if *pos + 4 > data.len() {
                bail!("Truncated payload");
            }
            *pos += 4;
        }
        _ => bail!("Unsupported protobuf wire type"),
    }
    Ok(())
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

    #[test]
    fn parses_migration_payload() {
        let payload = build_migration_payload(&[
            MigrationFixture {
                secret: b"Hello!\xde\xad\xbe\xef".to_vec(),
                name: "Example:alice@example.com",
                issuer: "Example",
                algorithm: 2,
                otp_type: 2,
                digits: 1,
            },
            MigrationFixture {
                secret: b"other-secret".to_vec(),
                name: "Foo:bar",
                issuer: "",
                algorithm: 1,
                otp_type: 1,
                digits: 2,
            },
        ]);
        let encoded = URL_SAFE_NO_PAD.encode(payload);
        let uri = format!("otpauth-migration://offline?data={encoded}");
        let entries = decode_qr_payload(&uri).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issuer, "Example");
        assert_eq!(entries[0].label, "alice@example.com");
        assert_eq!(entries[0].secret, encode_base32(b"Hello!\xde\xad\xbe\xef"));
        assert_eq!(entries[0].algorithm, Algorithm::Sha256);
        assert_eq!(entries[0].digits, 6);
    }

    #[test]
    fn parses_migration_payload_with_standard_base64_padding() {
        let payload = build_migration_payload(&[MigrationFixture {
            secret: b"hello-secret".to_vec(),
            name: "Example:alice@example.com",
            issuer: "Example",
            algorithm: 1,
            otp_type: 2,
            digits: 1,
        }]);
        let encoded = STANDARD.encode(payload);
        let uri = format!("otpauth-migration://offline?data={encoded}");
        let entries = decode_qr_payload(&uri).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issuer, "Example");
        assert_eq!(entries[0].secret, encode_base32(b"hello-secret"));
    }

    #[test]
    fn parses_migration_payload_with_url_safe_padded_base64() {
        let payload = build_migration_payload(&[MigrationFixture {
            secret: b"another-secret".to_vec(),
            name: "Issuer:user",
            issuer: "Issuer",
            algorithm: 1,
            otp_type: 2,
            digits: 1,
        }]);
        let encoded = URL_SAFE.encode(payload);
        let uri = format!("otpauth-migration://offline?data={encoded}");
        let entries = decode_qr_payload(&uri).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "user");
    }

    struct MigrationFixture {
        secret: Vec<u8>,
        name: &'static str,
        issuer: &'static str,
        algorithm: i32,
        otp_type: i32,
        digits: i32,
    }

    fn build_migration_payload(entries: &[MigrationFixture]) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in entries {
            let mut message = Vec::new();
            write_bytes_field(&mut message, 1, &entry.secret);
            write_string_field(&mut message, 2, entry.name.as_bytes());
            write_string_field(&mut message, 3, entry.issuer.as_bytes());
            write_varint_field(&mut message, 4, entry.algorithm as u64);
            write_varint_field(&mut message, 5, entry.digits as u64);
            write_varint_field(&mut message, 6, entry.otp_type as u64);
            write_bytes_field(&mut out, 1, &message);
        }
        out
    }

    fn write_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
        out.push(((field << 3) | 0) as u8);
        write_varint(out, value);
    }

    fn write_bytes_field(out: &mut Vec<u8>, field: u32, value: &[u8]) {
        out.push(((field << 3) | 2) as u8);
        write_varint(out, value.len() as u64);
        out.extend_from_slice(value);
    }

    fn write_string_field(out: &mut Vec<u8>, field: u32, value: &[u8]) {
        write_bytes_field(out, field, value);
    }

    fn write_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
                out.push(byte);
            } else {
                out.push(byte);
                break;
            }
        }
    }
}