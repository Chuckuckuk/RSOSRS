use crate::{fmt_err, xtea, Result};
use std::io::{Read, Write};

/// How [`encode`] packs a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
}

/// Encode `data` as a container (no XTEA, no trailing revision): the inverse of [`decode`].
pub fn encode(data: &[u8], compression: Compression) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 9);
    match compression {
        Compression::None => {
            out.push(0);
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        Compression::Gzip => {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            gz.write_all(data).expect("writing to memory");
            let payload = gz.finish().expect("writing to memory");
            out.push(2);
            out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(&payload);
        }
    }
    out
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoded_container_decodes_back() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        for c in [Compression::None, Compression::Gzip] {
            assert_eq!(decode(&encode(&data, c), None).unwrap(), data, "{c:?}");
            assert_eq!(decode(&encode(&[], c), None).unwrap(), Vec::<u8>::new(), "{c:?} empty");
        }
    }
}
