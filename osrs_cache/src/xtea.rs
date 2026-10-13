pub type Key = [u32; 4];

const DELTA: u32 = 0x9E37_79B9;

/// Decrypt whole 8-byte blocks in place (a trailing partial block is left as is).
pub fn decrypt(data: &mut [u8], key: &Key) {
    for block in data.chunks_exact_mut(8) {
        let mut v0 = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut v1 = u32::from_be_bytes(block[4..8].try_into().unwrap());
        let mut sum = DELTA.wrapping_mul(32);
        for _ in 0..32 {
            v1 = v1.wrapping_sub(
                (((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0)) ^ sum.wrapping_add(key[((sum >> 11) & 3) as usize]),
            );
            sum = sum.wrapping_sub(DELTA);
            v0 = v0.wrapping_sub(
                (((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1)) ^ sum.wrapping_add(key[(sum & 3) as usize]),
            );
        }
        block[0..4].copy_from_slice(&v0.to_be_bytes());
        block[4..8].copy_from_slice(&v1.to_be_bytes());
    }
}
