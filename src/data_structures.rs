use crate::link::{EK, PP, VK};
use ark_ec::PairingEngine;
use ark_ff::bytes::ToBytes;
use ark_serialize::*;
use ark_std::{
    io::{self, Result as IoResult},
    vec::Vec,
};

/// A proof in the Groth16 SNARK
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct Proof<E: PairingEngine> {
    /// The `A` element in `G1`.
    pub a: E::G1Affine,
    /// The `B` element in `G2`.
    pub b: E::G2Affine,
    /// The `C` element in `G1`.
    pub c: E::G1Affine,
    /// The `D` element in `G1`. Commits to a subset of private inputs of the circuit
    pub d: E::G1Affine,
}

/// A proof in the Groth16 SNARK with CP_link proof
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct ProofWithLink<E: PairingEngine> {
    pub groth16_proof: Proof<E>,
    /// cp_{link}
    pub link_d: E::G1Affine,
    /// proof of commitment opening equality between `cp_{link}` and `d`
    pub link_pi: E::G1Affine,
}

impl<E: PairingEngine> ToBytes for Proof<E> {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> io::Result<()> {
        self.a.write(&mut writer)?;
        self.b.write(&mut writer)?;
        self.c.write(&mut writer)?;
        self.d.write(&mut writer)
    }
}

impl<E: PairingEngine> Default for Proof<E> {
    fn default() -> Self {
        Self {
            a: E::G1Affine::default(),
            b: E::G2Affine::default(),
            c: E::G1Affine::default(),
            d: E::G1Affine::default(),
        }
    }
}

impl<E: PairingEngine> ToBytes for ProofWithLink<E> {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> io::Result<()> {
        self.groth16_proof.write(&mut writer)?;
        self.link_d.write(&mut writer)?;
        self.link_pi.write(&mut writer)
    }
}

impl<E: PairingEngine> Default for ProofWithLink<E> {
    fn default() -> Self {
        Self {
            groth16_proof: Proof::default(),
            link_pi: E::G1Affine::default(),
            link_d: E::G1Affine::default(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

/// A verification key in the Groth16 SNARK.
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct VerifyingKey<E: PairingEngine> {
    /// The `alpha * G`, where `G` is the generator of `E::G1`.
    pub alpha_g1: E::G1Affine,
    /// The `alpha * H`, where `H` is the generator of `E::G2`.
    pub beta_g2: E::G2Affine,
    /// The `gamma * H`, where `H` is the generator of `E::G2`.
    pub gamma_g2: E::G2Affine,
    /// The `delta * H`, where `H` is the generator of `E::G2`.
    pub delta_g2: E::G2Affine,
    /// The `gamma^{-1} * (beta * a_i + alpha * b_i + c_i) * H`, where `H` is the generator of `E::G1`.
    pub gamma_abc_g1: Vec<E::G1Affine>,
    /// The element `eta*gamma^-1 * G` in `E::G1`.
    pub eta_gamma_inv_g1: E::G1Affine,
    /// No of witness to commit
    pub commit_witness_count: usize,
}

/// A verification key in the Groth16 SNARK with CP_link verification parameters
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct VerifyingKeyWithLink<E: PairingEngine> {
    pub groth16_vk: VerifyingKey<E>,
    /// Public parameters of the Subspace Snark
    pub link_pp: PP<E::G1Affine, E::G2Affine>,
    /// Commitment key of the link commitment cp_link
    pub link_bases: Vec<E::G1Affine>,
    /// Verification key of the Subspace Snark
    pub link_vk: VK<E::G2Affine>,
}

impl<E: PairingEngine> ToBytes for VerifyingKey<E> {
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        self.alpha_g1.write(&mut writer)?;
        self.beta_g2.write(&mut writer)?;
        self.gamma_g2.write(&mut writer)?;
        self.delta_g2.write(&mut writer)?;
        for q in &self.gamma_abc_g1 {
            q.write(&mut writer)?;
        }
        self.eta_gamma_inv_g1.write(&mut writer)
    }
}

impl<E: PairingEngine> Default for VerifyingKey<E> {
    fn default() -> Self {
        Self {
            alpha_g1: E::G1Affine::default(),
            beta_g2: E::G2Affine::default(),
            gamma_g2: E::G2Affine::default(),
            delta_g2: E::G2Affine::default(),
            gamma_abc_g1: Vec::new(),
            eta_gamma_inv_g1: E::G1Affine::default(),
            commit_witness_count: 0,
        }
    }
}

impl<E: PairingEngine> ToBytes for VerifyingKeyWithLink<E> {
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        self.groth16_vk.write(&mut writer)?;
        self.link_pp.write(&mut writer)?;
        for q in &self.link_bases {
            q.write(&mut writer)?;
        }
        self.link_vk.write(&mut writer)
    }
}

impl<E: PairingEngine> Default for VerifyingKeyWithLink<E> {
    fn default() -> Self {
        Self {
            groth16_vk: VerifyingKey::default(),
            link_pp: PP::<E::G1Affine, E::G2Affine>::default(),
            link_bases: Vec::new(),
            link_vk: VK::<E::G2Affine>::default(),
        }
    }
}

/// Preprocessed verification key parameters that enable faster verification
/// at the expense of larger size in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedVerifyingKey<E: PairingEngine> {
    /// The unprepared verification key.
    pub vk: VerifyingKey<E>,
    /// The element `e(alpha * G, beta * H)` in `E::GT`.
    pub alpha_g1_beta_g2: E::Fqk,
    /// The element `- gamma * H` in `E::G2`, prepared for use in pairings.
    pub gamma_g2_neg_pc: E::G2Prepared,
    /// The element `- delta * H` in `E::G2`, prepared for use in pairings.
    pub delta_g2_neg_pc: E::G2Prepared,
}

impl<E: PairingEngine> From<PreparedVerifyingKey<E>> for VerifyingKey<E> {
    fn from(other: PreparedVerifyingKey<E>) -> Self {
        other.vk
    }
}

impl<E: PairingEngine> From<&VerifyingKey<E>> for PreparedVerifyingKey<E> {
    fn from(other: &VerifyingKey<E>) -> Self {
        crate::prepare_verifying_key(other)
    }
}

impl<E: PairingEngine> Default for PreparedVerifyingKey<E> {
    fn default() -> Self {
        Self {
            vk: VerifyingKey::default(),
            alpha_g1_beta_g2: E::Fqk::default(),
            gamma_g2_neg_pc: E::G2Prepared::default(),
            delta_g2_neg_pc: E::G2Prepared::default(),
        }
    }
}

impl<E: PairingEngine> ToBytes for PreparedVerifyingKey<E> {
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        self.vk.write(&mut writer)?;
        self.alpha_g1_beta_g2.write(&mut writer)?;
        self.gamma_g2_neg_pc.write(&mut writer)?;
        self.delta_g2_neg_pc.write(&mut writer)?;
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

/// The common elements for Proving Key for with and without CP_link
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct ProvingKeyCommon<E: PairingEngine> {
    /// The element `beta * G` in `E::G1`.
    pub beta_g1: E::G1Affine,
    /// The element `delta * G` in `E::G1`.
    pub delta_g1: E::G1Affine,
    /// The element `eta*delta^-1 * G` in `E::G1`.
    pub eta_delta_inv_g1: E::G1Affine,
    /// The elements `a_i * G` in `E::G1`.
    pub a_query: Vec<E::G1Affine>,
    /// The elements `b_i * G` in `E::G1`.
    pub b_g1_query: Vec<E::G1Affine>,
    /// The elements `b_i * H` in `E::G2`.
    pub b_g2_query: Vec<E::G2Affine>,
    /// The elements `h_i * G` in `E::G1`.
    pub h_query: Vec<E::G1Affine>,
    /// The elements `l_i * G` in `E::G1`.
    pub l_query: Vec<E::G1Affine>,
}

/// The prover key for for the Groth16 zkSNARK.
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct ProvingKey<E: PairingEngine> {
    /// The underlying verification key.
    pub vk: VerifyingKey<E>,
    pub common: ProvingKeyCommon<E>,
}

/// The prover key for for the Groth16 zkSNARK with CP_link parameters
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct ProvingKeyWithLink<E: PairingEngine> {
    /// The underlying verification key.
    pub vk: VerifyingKeyWithLink<E>,
    pub common: ProvingKeyCommon<E>,
    /// Evaluation key of cp_{link}
    pub link_ek: EK<E::G1Affine>,
}

/// Public parameters for CP link
#[derive(Clone, Debug, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct LinkPublicGenerators<E: PairingEngine> {
    pub pedersen_gens: Vec<E::G1Affine>,
    pub g1: E::G1Affine,
    pub g2: E::G2Affine,
}

impl<E: PairingEngine> VerifyingKey<E> {
    pub fn num_public_inputs(&self) -> usize {
        self.gamma_abc_g1.len() - self.commit_witness_count
    }

    pub fn num_committed_witnesses(&self) -> usize {
        self.commit_witness_count
    }

    pub fn get_commitment_key_for_witnesses(&self) -> Vec<E::G1Affine> {
        let start = self.num_public_inputs();
        let end = start + self.commit_witness_count;
        let mut key = Vec::with_capacity(self.commit_witness_count + 1);
        key.extend_from_slice(&self.gamma_abc_g1[start..end]);
        key.push(self.eta_gamma_inv_g1);
        key
    }
}
