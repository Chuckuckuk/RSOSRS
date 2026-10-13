use crate::{fmt_err, xtea, Result};
use std::io::Read;

/// Decode a container: `[compression u8][len u32][ulen u32 if compressed][data][revision u16?]`.
/// XTEA (when a key is given) covers everything after the 5-byte header.
pub fn decode(raw: &[u8], key: Option<&xtea::Key>) -> Result<Vec<u8>> {
    if raw.len() < 5 {
        return fmt_err("container too short");
    }
    let comp = raw[0];
    let len = u32::from_be_bytes(raw[1..5].try_into().unwrap()) as usize;
    let end = 5 + len + if comp == 0 { 0 } else { 4 };
    if raw.len() < end {
        return fmt_err(format!("container truncated: have {} need {}", raw.len(), end));
    }
    let mut body = raw[5..end].to_vec();
    if let Some(k) = key {
        xtea::decrypt(&mut body, k);
    }
    match comp {
        0 => Ok(body),
        1 | 2 => {
            let ulen = u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize;
            let payload = &body[4..];
            let mut out = Vec::with_capacity(ulen);
            let res = if comp == 1 {
                // Jagex strips the "BZh1" magic.
                let mut with_magic = Vec::with_capacity(payload.len() + 4);
                with_magic.extend_from_slice(b"BZh1");
                with_magic.extend_from_slice(payload);
                bzip2::read::BzDecoder::new(&with_magic[..]).read_to_end(&mut out)
            } else {
                flate2::read::GzDecoder::new(payload).read_to_end(&mut out)
            };
            if res.is_err() || out.len() != ulen {
                return fmt_err(format!(
                    "decompression failed ({} of {} bytes; wrong xtea key?)",
                    out.len(),
                    ulen
                ));
            }
            Ok(out)
        }
        c => fmt_err(format!("unknown compression {c}")),
    }
}
