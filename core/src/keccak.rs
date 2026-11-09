use tiny_keccak::{Hasher, Keccak};

/// keccak256 of the concatenated parts, as Solidity's `keccak256(abi.encodePacked(...))`
/// computes it for byte inputs (README §6.4).
pub fn keccak256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    for part in parts {
        hasher.update(part);
    }
    let mut hash = [0; 32];
    hasher.finalize(&mut hash);
    hash
}
