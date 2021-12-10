use crate::{Proof, VerifyingKey};
use ark_ec::msm::VariableBaseMSM;
use ark_ec::{AffineCurve, PairingEngine, ProjectiveCurve};
use ark_ff::PrimeField;
use ark_relations::r1cs::SynthesisError;
use ark_std::ops::AddAssign;

/// Redact public inputs from the commitment in the proof such that commitment opens only to the witnesses
pub fn get_commitment_to_witnesses<E: PairingEngine>(
    vk: &VerifyingKey<E>,
    proof: &Proof<E>,
    public_inputs: &[E::Fr],
) -> Result<E::G1Affine, SynthesisError> {
    // TODO: Use errors for size checks
    let inputs = public_inputs
        .into_iter()
        .map(|p| p.into_repr())
        .collect::<Vec<_>>();
    let mut g_link = vk.link_bases[0].into_projective();
    g_link.add_assign(VariableBaseMSM::multi_scalar_mul(
        &vk.link_bases[1..],
        &inputs,
    ));
    Ok((proof.link_d.into_projective() - g_link).into_affine())
}

pub fn verify_link_commitment<E: PairingEngine>(
    vk: &VerifyingKey<E>,
    proof: &Proof<E>,
    public_inputs: &[E::Fr],
    witnesses_expected_in_commitment: &[E::Fr],
    link_v: &E::Fr,
) -> Result<bool, SynthesisError> {
    // TODO: Handle errors on size mismatch
    let inputs = public_inputs
        .iter()
        .chain(witnesses_expected_in_commitment.iter())
        .map(|p| p.into_repr())
        .collect::<Vec<_>>();

    let mut g_link = vk.link_bases[0].into_projective();
    g_link.add_assign(VariableBaseMSM::multi_scalar_mul(
        &vk.link_bases[1..],
        &inputs,
    ));
    g_link.add_assign(&vk.link_bases.last().unwrap().mul(link_v.into_repr()));
    Ok(proof.link_d == g_link.into_affine())
}

/// Check that the commitments in the proof open to the public inputs and the witnesses but with different
/// bases and randomness
pub fn verify_commitment_new<E: PairingEngine>(
    vk: &VerifyingKey<E>,
    proof: &Proof<E>,
    public_inputs: &[E::Fr],
    witnesses_expected_in_commitment: &[E::Fr],
    v: &E::Fr,
    link_v: &E::Fr,
) -> Result<bool, SynthesisError> {
    let inputs = public_inputs
        .iter()
        .chain(witnesses_expected_in_commitment.iter())
        .map(|p| p.into_repr())
        .collect::<Vec<_>>();

    let mut g_ic = vk.gamma_abc_g1[0].into_projective();
    g_ic.add_assign(VariableBaseMSM::multi_scalar_mul(
        &vk.gamma_abc_g1[1..],
        &inputs,
    ));
    g_ic.add_assign(&vk.eta_gamma_inv_g1.mul(v.into_repr()));

    let r1 = proof.d == g_ic.into_affine();
    let r2 = verify_link_commitment(
        vk,
        proof,
        public_inputs,
        witnesses_expected_in_commitment,
        link_v,
    )?;
    // println!("{} {}", r1, r2);
    // TODO: Return error indicating which check failed
    Ok(r1 && r2)
}
