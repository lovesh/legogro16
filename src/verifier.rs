use crate::link::{PESubspaceSnark, SubspaceSnark};
use ark_ec::{AffineCurve, PairingEngine, ProjectiveCurve};
use ark_ff::PrimeField;

use super::{PreparedVerifyingKey, Proof, VerifyingKey};

use ark_relations::r1cs::SynthesisError;

use crate::error::Error;
use ark_std::vec;
use ark_std::vec::Vec;
use core::ops::{AddAssign, Neg};

/// Prepare the verifying key `vk` for use in proof verification.
pub fn prepare_verifying_key<E: PairingEngine>(vk: &VerifyingKey<E>) -> PreparedVerifyingKey<E> {
    PreparedVerifyingKey {
        vk: vk.clone(),
        alpha_g1_beta_g2: E::pairing(vk.alpha_g1, vk.beta_g2),
        gamma_g2_neg_pc: vk.gamma_g2.neg().into(),
        delta_g2_neg_pc: vk.delta_g2.neg().into(),
    }
}

/// Prepare proof inputs for use with [`verify_proof_with_prepared_inputs`], wrt the prepared
/// verification key `pvk` and instance public inputs.
pub fn prepare_inputs<E: PairingEngine>(
    pvk: &PreparedVerifyingKey<E>,
    public_inputs: &[E::Fr],
) -> crate::Result<E::G1Projective> {
    if (public_inputs.len() + 1) > pvk.vk.gamma_abc_g1.len() {
        return Err(SynthesisError::MalformedVerifyingKey).map_err(|e| e.into());
    }

    let mut d = pvk.vk.gamma_abc_g1[0].into_projective();
    for (i, b) in public_inputs.iter().zip(pvk.vk.gamma_abc_g1.iter().skip(1)) {
        d.add_assign(&b.mul(i.into_repr()));
    }

    Ok(d)
}

/// Verify the proof of the Subspace Snark on the equality of openings of cp_link and proof.d
pub fn verify_link_proof<E: PairingEngine>(
    vk: &VerifyingKey<E>,
    proof: &Proof<E>,
) -> crate::Result<()> {
    let commitments = vec![proof.link_d.into_projective(), proof.d.into_projective()];
    PESubspaceSnark::<E>::verify(
        &vk.link_pp,
        &vk.link_vk,
        &commitments
            .iter()
            .map(|p| p.into_affine())
            .collect::<Vec<_>>(),
        &proof.link_pi,
    )
    .map_err(|e| e.into())
}

pub fn verify_qap_proof<E: PairingEngine>(
    pvk: &PreparedVerifyingKey<E>,
    a: E::G1Affine,
    b: E::G2Affine,
    c: E::G1Affine,
    d: E::G1Affine,
) -> crate::Result<()> {
    let qap = E::miller_loop(
        [
            (a.into(), b.into()),
            (c.into(), pvk.delta_g2_neg_pc.clone()),
            (d.into(), pvk.gamma_g2_neg_pc.clone()),
        ]
        .iter(),
    );

    if E::final_exponentiation(&qap).ok_or(SynthesisError::UnexpectedIdentity)?
        != pvk.alpha_g1_beta_g2
    {
        return Err(Error::InvalidProof);
    }
    Ok(())
}

/// Verify a LegoGroth16 proof `proof` against the prepared verification key `pvk`
pub fn verify_proof<E: PairingEngine>(
    pvk: &PreparedVerifyingKey<E>,
    proof: &Proof<E>,
    public_inputs: &[E::Fr],
) -> crate::Result<()> {
    verify_link_proof(&pvk.vk, proof)?;
    let mut d = proof.d.into_projective();
    d.add_assign(prepare_inputs(pvk, public_inputs)?);

    verify_qap_proof(pvk, proof.a, proof.b, proof.c, d.into_affine())
}
