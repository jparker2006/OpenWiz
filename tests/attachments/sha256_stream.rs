//! The streaming SHA-256 (`Sha256Hasher`) that staging hashes with: the
//! same hash as `sha256_hex` of the whole input, however the bytes are cut
//! into pieces, and the FIPS 180-4 examples.

use openwiz_core::messaging::{Sha256Hasher, sha256_hex};

fn streamed(pieces: &[&[u8]]) -> String {
    let mut hasher = Sha256Hasher::new();
    for piece in pieces {
        hasher.update(piece);
    }
    hasher.finish()
}

#[test]
fn the_fips_examples() {
    for (input, hex) in [
        (
            &b""[..],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ] {
        assert_eq!(streamed(&[input]), hex);
        assert_eq!(sha256_hex(input), hex);
    }
    let million = vec![b'a'; 1_000_000];
    let hex = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
    assert_eq!(sha256_hex(&million), hex);
    let pieces: Vec<&[u8]> = million.chunks(997).collect();
    assert_eq!(streamed(&pieces), hex);
}

#[test]
fn every_cut_gives_the_whole_inputs_hash() {
    let data: Vec<u8> = (0..300u32).map(|i| (i * 31 % 251) as u8).collect();
    for len in 0..data.len() {
        let whole = sha256_hex(&data[..len]);
        assert_eq!(streamed(&[&data[..len]]), whole, "{len}");
        for cut in 0..=len {
            assert_eq!(
                streamed(&[&data[..cut], &data[cut..len]]),
                whole,
                "{len} cut at {cut}"
            );
        }
        let bytes: Vec<&[u8]> = data[..len].chunks(1).collect();
        assert_eq!(streamed(&bytes), whole, "{len} a byte at a time");
    }
    assert_eq!(Sha256Hasher::default().finish(), sha256_hex(b""));
}
