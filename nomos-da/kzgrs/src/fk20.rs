use crate::{GlobalParameters, Polynomial, Proof};
use ark_bls12_381::{Fr, G1Affine, G1Projective};
use ark_ec::CurveGroup;
use ark_ff::{Field, UniformRand};
use ark_poly::{EvaluationDomain, GeneralEvaluationDomain};
use num_traits::Zero;
use rand::rngs::OsRng;
use std::borrow::Cow;
use std::ops::Mul;

fn toeplitz1(global_parameters: &[G1Affine], polynomial_degree: usize) -> Vec<G1Projective> {
    debug_assert_eq!(global_parameters.len(), polynomial_degree);
    debug_assert!(polynomial_degree.is_power_of_two());
    let domain: GeneralEvaluationDomain<Fr> = GeneralEvaluationDomain::new(polynomial_degree * 2)
        .expect("Domain should be able to build");
    let vector_extended: Vec<G1Projective> = global_parameters
        .iter()
        .copied()
        .map(G1Projective::from)
        .chain(std::iter::repeat_with(G1Projective::zero).take(polynomial_degree))
        .collect();
    domain.fft(&vector_extended)
}

fn toeplitz2(coefficients: &[Fr], extended_vector: &[G1Projective]) -> Vec<G1Projective> {
    debug_assert!(coefficients.len().is_power_of_two());
    let domain: GeneralEvaluationDomain<Fr> =
        GeneralEvaluationDomain::new(coefficients.len()).expect("Domain should be able to build");
    let toeplitz_coefficients_fft = domain.fft(coefficients);
    extended_vector
        .iter()
        .zip(toeplitz_coefficients_fft)
        .map(|(v, c)| (v.mul(c)))
        .collect()
}

fn toeplitz3(mut h_extended_fft: Vec<G1Projective>) -> Vec<G1Projective> {
    let domain: GeneralEvaluationDomain<Fr> =
        GeneralEvaluationDomain::new(h_extended_fft.len()).expect("Domain should be able to build");

    domain.ifft_in_place(&mut h_extended_fft);
    h_extended_fft
}

pub fn fk20_batch_generate_elements_proofs(
    polynomial: &Polynomial,
    global_parameters: &GlobalParameters,
    toeplitz1_cache: Option<&Toeplitz1Cache>,
) -> Vec<Proof> {
    let polynomial_degree = polynomial.len();
    debug_assert!(polynomial_degree <= global_parameters.powers_of_g.len());
    debug_assert!(polynomial_degree.is_power_of_two());
    let domain: GeneralEvaluationDomain<Fr> =
        GeneralEvaluationDomain::new(polynomial_degree).expect("Domain should be able to build");

    let extended_vector = if let Some(Toeplitz1Cache(v)) = toeplitz1_cache {
        Cow::Borrowed(v)
    } else {
        let global_parameters: Vec<G1Affine> = global_parameters
            .powers_of_g
            .iter()
            .copied()
            .take(polynomial_degree)
            .rev()
            .collect();
        Cow::Owned(toeplitz1(&global_parameters, polynomial_degree))
    };
    let toeplitz_coefficients: Vec<Fr> = std::iter::repeat(Fr::ZERO)
        .take(polynomial_degree)
        .chain(polynomial.coeffs.iter().copied())
        .collect();
    let h_extended_vector = toeplitz2(&toeplitz_coefficients, &extended_vector);
    let h_vector = toeplitz3(h_extended_vector);

    // Initialize a random number generator
    let mut rng = OsRng;

    domain
        .fft(&h_vector)
        .into_iter()
        .map(|g1| {
            // Generate a random field element
            let random_v = Fr::rand(&mut rng);

            // Adjust 'w' using 'random_v'
            // w = w + random_v * G
            let generator_g1 = global_parameters.powers_of_g[0]; // Assuming the first element is the generator G
            let adjusted_g1 = g1 + generator_g1.mul(random_v);

            Proof {
                w: adjusted_g1.into_affine(),
                random_v: Some(random_v),
            }
        })
        .collect()
}

#[derive(Clone)]
pub struct Toeplitz1Cache(Vec<G1Projective>);

impl Toeplitz1Cache {
    pub fn with_size(global_parameters: &GlobalParameters, polynomial_degree: usize) -> Self {
        let global_parameters: Vec<G1Affine> = global_parameters
            .powers_of_g
            .iter()
            .copied()
            .take(polynomial_degree)
            .rev()
            .collect();
        Self(toeplitz1(&global_parameters, polynomial_degree))
    }
}

#[cfg(test)]
mod test {
    use crate::fk20::{fk20_batch_generate_elements_proofs, Toeplitz1Cache};
    use crate::{
        common::bytes_to_polynomial, kzg::generate_element_proof, GlobalParameters, Proof,
        BYTES_PER_FIELD_ELEMENT,
    };
    use ark_bls12_381::{Bls12_381, Fr};
    use ark_poly::univariate::DensePolynomial;
    use ark_poly::{EvaluationDomain, GeneralEvaluationDomain};
    use ark_poly_commit::kzg10::KZG10;
    use once_cell::sync::Lazy;
    use rand::SeedableRng;

    static GLOBAL_PARAMETERS: Lazy<GlobalParameters> = Lazy::new(|| {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1987);
        KZG10::<Bls12_381, DensePolynomial<Fr>>::setup(4096, true, &mut rng).unwrap()
    });

    #[test]
    fn test_generate_proofs() {
        for size in [16, 32, 64, 128, 256] {
            let buff: Vec<_> = (0..BYTES_PER_FIELD_ELEMENT * size)
                .map(|i| (i % 255) as u8)
                .rev()
                .collect();
            let domain = GeneralEvaluationDomain::new(size).unwrap();
            let (evals, poly) =
                bytes_to_polynomial::<BYTES_PER_FIELD_ELEMENT>(&buff, domain).unwrap();
            let polynomial_degree = poly.len();
            let slow_proofs: Vec<Proof> = (0..polynomial_degree)
                .map(|i| {
                    generate_element_proof(i, &poly, &evals, &GLOBAL_PARAMETERS, domain).unwrap()
                })
                .collect();
            let fk20_proofs = fk20_batch_generate_elements_proofs(&poly, &GLOBAL_PARAMETERS, None);
            assert_eq!(slow_proofs, fk20_proofs);

            // Test variant with Toeplitz1Cache param
            let tc = Toeplitz1Cache::with_size(&GLOBAL_PARAMETERS.clone(), size);
            let fk20_proofs =
                fk20_batch_generate_elements_proofs(&poly, &GLOBAL_PARAMETERS, Some(&tc));
            assert_eq!(slow_proofs, fk20_proofs);
        }
    }
}
