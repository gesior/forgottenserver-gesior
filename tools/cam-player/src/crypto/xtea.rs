pub type Key = [u32; 4];
pub type RoundKeys = [u32; 64];

pub fn expand_key(k: &Key) -> RoundKeys {
    const DELTA: u32 = 0x9E3779B9;
    let mut expanded = [0u32; 64];
    let mut sum: u32 = 0;
    let mut next_sum: u32 = sum.wrapping_add(DELTA);

    let mut i = 0;
    while i < 64 {
        expanded[i] = sum.wrapping_add(k[(sum & 3) as usize]);
        expanded[i + 1] = next_sum.wrapping_add(k[((next_sum >> 11) & 3) as usize]);
        sum = next_sum;
        next_sum = next_sum.wrapping_add(DELTA);
        i += 2;
    }

    expanded
}

pub fn encrypt(data: &mut [u8], k: &RoundKeys) {
    assert_eq!(data.len() % 8, 0, "Data length must be multiple of 8");
    for chunk in data.chunks_exact_mut(8) {
        let mut left = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let mut right = u32::from_le_bytes(chunk[4..8].try_into().unwrap());

        let mut i = 0;
        while i < 64 {
            let part1 = ((right << 4) ^ (right >> 5)).wrapping_add(right);
            left = left.wrapping_add(part1 ^ k[i]);

            let part2 = ((left << 4) ^ (left >> 5)).wrapping_add(left);
            right = right.wrapping_add(part2 ^ k[i + 1]);

            i += 2;
        }

        chunk[0..4].copy_from_slice(&left.to_le_bytes());
        chunk[4..8].copy_from_slice(&right.to_le_bytes());
    }
}

pub fn decrypt(data: &mut [u8], k: &RoundKeys) {
    assert_eq!(data.len() % 8, 0, "Data length must be multiple of 8");
    for chunk in data.chunks_exact_mut(8) {
        let mut left = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let mut right = u32::from_le_bytes(chunk[4..8].try_into().unwrap());

        let mut i = 64;
        while i > 0 {
            let part2 = ((left << 4) ^ (left >> 5)).wrapping_add(left);
            right = right.wrapping_sub(part2 ^ k[i - 1]);

            let part1 = ((right << 4) ^ (right >> 5)).wrapping_add(right);
            left = left.wrapping_sub(part1 ^ k[i - 2]);

            i -= 2;
        }

        chunk[0..4].copy_from_slice(&left.to_le_bytes());
        chunk[4..8].copy_from_slice(&right.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xtea_roundtrip() {
        let key: Key = [0x12345678, 0x9ABCDEF0, 0x0FEDCBA9, 0x87654321];
        let rk = expand_key(&key);
        let original = b"Hello, Tibia 1098 World!".to_vec(); // 24 bytes (multiple of 8)
        let mut buffer = original.clone();

        encrypt(&mut buffer, &rk);
        assert_ne!(buffer, original);

        decrypt(&mut buffer, &rk);
        assert_eq!(buffer, original);
    }
}
