use ed25519_dalek::SigningKey;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let hex_str = &args[1];
    let bytes = hex::decode(hex_str).unwrap();
    let bytes: [u8; 32] = bytes.try_into().unwrap();
    let signing_key = SigningKey::from_bytes(&bytes);
    println!("{}", hex::encode(signing_key.verifying_key().to_bytes()));
}
