use oqs::kem::{Algorithm as KemAlg, Kem};
use oqs::sig::{Algorithm as SigAlg, Sig};

fn main() {
    let kem_alg = KemAlg::MlKem1024;
    let sig_alg = SigAlg::MlDsa87;
    println!("KEM: {:?}, SIG: {:?}", kem_alg, sig_alg);
}
