use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let pub_hex = &args[1];
    let sig_hex = &args[2];
    let msg = &args[3];

    let pub_bytes = hex::decode(pub_hex).unwrap();
    let pub_bytes: [u8; 32] = pub_bytes.try_into().unwrap();
    let verifying_key = VerifyingKey::from_bytes(&pub_bytes).unwrap();

    let sig_bytes = hex::decode(sig_hex).unwrap();
    let sig_bytes: [u8; 64] = sig_bytes.try_into().unwrap();
    let signature = Signature::from_bytes(&sig_bytes);

    match verifying_key.verify(msg.as_bytes(), &signature) {
        Ok(_) => println!("Valid!"),
        Err(e) => println!("Invalid: {}", e),
    }
}
