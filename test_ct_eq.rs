use subtle::ConstantTimeEq;

fn main() {
    let expected_hmac: [u8; 32] = [0; 32];
    let actual_hmac: Vec<u8> = vec![0; 32];

    let is_match = if actual_hmac.len() != expected_hmac.len() {
        let _ = expected_hmac.ct_eq(&expected_hmac);
        false
    } else {
        bool::from(expected_hmac.ct_eq(actual_hmac.as_slice()))
    };
    println!("is_match: {}", is_match);
}
