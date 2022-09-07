//! Largely copied from <https://github.com/gakonst/ark-circom/blob/master/src/witness/witness_calculator.rs>
//! And some more checks defined in <https://github.com/iden3/circom_runtime/blob/master/js/witness_calculator.js>

use ark_ec::PairingEngine;
use ark_ff::{BigInteger, FpParameters, PrimeField};
use ark_std::iter::IntoIterator;
use ark_std::marker::PhantomData;
use ark_std::ops::MulAssign;
use ark_std::{format, string::String, string::ToString, vec, vec::Vec};
use core::hash::Hasher;
use fnv::FnvHasher;
use num_bigint::BigUint;
use wasmer::{imports, Instance, Module, Store};

use crate::circom::{BLS12_381_ORDER, BN128_ORDER};

use crate::circom::error::CircomError;
use crate::circom::r1cs::Curve;
use crate::circom::wasm::Wasm;

/// Used to calculates the values of the wires of a circuit given its WASM generated by Circom.
#[derive(Clone, Debug)]
pub struct WitnessCalculator<E: PairingEngine> {
    pub instance: Wasm,
    pub circom_version: u32,
    pub curve: Curve,
    phantom: PhantomData<E>,
}

impl<E: PairingEngine> WitnessCalculator<E> {
    /// Create the WASM module using the WASM file generated by Circom
    #[cfg(feature = "std")]
    pub fn from_wasm_file(path: impl AsRef<std::path::Path>) -> Result<Self, CircomError> {
        let store = Store::default();
        let module = Module::from_file(&store, path).map_err(|err| {
            log::error!(
                "Encountered error while loading WASM module from file: {:?}",
                err
            );
            CircomError::UnableToLoadWasmModuleFromFile(format!(
                "Encountered error while loading WASM module from file: {:?}",
                err
            ))
        })?;
        Self::from_module(module)
    }

    /// Create the WASM module using the bytes of the WASM file generated by Circom
    pub fn from_wasm_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, CircomError> {
        let store = Store::default();
        let module = Module::new(&store, bytes).map_err(|err| {
            log::error!(
                "Encountered error while loading WASM module from file: {:?}",
                err
            );
            CircomError::UnableToLoadWasmModuleFromBytes(format!(
                "Encountered error while loading WASM module from bytes: {:?}",
                err
            ))
        })?;
        Self::from_module(module)
    }

    /// Initialize using the WASM module generated by Circom.
    pub fn from_module(module: Module) -> Result<Self, CircomError> {
        let store = module.store();

        // Set up the memory
        let import_object = imports! {
            // Host function callbacks from the WASM
            "runtime" => {
                "exceptionHandler" => runtime::exception_handler(store),
                "showSharedRWMemory" => runtime::show_memory(store),
                "printErrorMessage" => runtime::print_error_message(store),
                "writeBufferMessage" => runtime::write_buffer_message(store),
            }
        };

        let instance = Wasm::new(Instance::new(&module, &import_object).map_err(|err| {
            log::error!(
                "Encountered error while instantiating WASM module: {:?}",
                err
            );
            CircomError::WasmInstantiationError(format!(
                "Encountered error while instantiating WASM module: {:?}",
                err
            ))
        })?);
        let version = instance.get_version()?;
        if version != 2 {
            return Err(CircomError::UnsupportedVersion(version));
        }

        // Read the order of the group
        let n32 = instance.get_field_num_len32()?;
        instance.get_raw_prime()?;
        let mut order_bytes = vec![0u8; (n32 * 4) as usize];
        for i in 0..n32 {
            let res = instance.read_shared_rw_memory(i)?;
            for j in 0..4 {
                order_bytes[(i * 4 + j) as usize] = ((res >> (8 * j)) & 255) as u8;
            }
        }

        let curve = check_subgroup_order::<E>(&order_bytes)?;

        Ok(WitnessCalculator {
            instance,
            circom_version: version,
            curve,
            phantom: PhantomData,
        })
    }

    /// Given the input wires (signals), calculate the values of the remaining wires and return the
    /// values of all wires of the circuit. The input wires are a map from the signal name to its
    /// value (values if the signal is an array). The returned wire list will always have 1st wire
    /// with value "1", followed by values of output wires, then the input wires. The order of input
    /// wires in this list is the same in which the got created in the circuit.
    pub fn calculate_witnesses<I: IntoIterator<Item = (String, Vec<E::Fr>)>>(
        &mut self,
        inputs: I,
        sanity_check: bool,
    ) -> Result<Vec<E::Fr>, CircomError> {
        self.instance.init(sanity_check)?;
        // Field element size in 32-byte chunks
        let field_element_size = self.instance.get_field_num_len32()?;

        let mut seen_inputs = 0;
        // allocate the inputs
        for (name, values) in inputs.into_iter() {
            let (msb, lsb) = fnv(&name);

            let mut seen_signals = 0;
            for (i, value) in values.into_iter().enumerate() {
                let f_arr = to_array32::<E>(&value, field_element_size as usize);
                for j in 0..field_element_size {
                    self.instance
                        .write_shared_rw_memory(j as u32, f_arr[j as usize])?;
                }
                self.instance
                    .set_input_signal(msb as u32, lsb as u32, i as u32)?;
                seen_inputs += 1;
                seen_signals += 1;
            }
            let required_signals = self.instance.get_signal_count(msb, lsb)?;
            if required_signals != seen_signals {
                return Err(CircomError::IncorrectNumberOfSignalsProvided(
                    name.to_string(),
                    required_signals,
                    seen_signals,
                ));
            }
        }

        let required_inputs = self.instance.get_input_count()?;
        if required_inputs != seen_inputs {
            return Err(CircomError::IncorrectNumberOfInputsProvided(
                required_inputs,
                seen_inputs,
            ));
        }

        let mut wires = Vec::new();

        let witness_size = self.instance.get_witness_count()?;
        for i in 0..witness_size {
            self.instance.get_witness(i)?;
            let mut arr = vec![0; field_element_size as usize];
            for j in 0..field_element_size {
                // Reading in little endian with read_shared_rw_memory
                arr[j as usize] = self.instance.read_shared_rw_memory(j)?;
            }
            wires.push(from_array32::<E>(arr));
        }

        Ok(wires)
    }
}

// callback hooks for debugging
mod runtime {
    use super::*;
    use wasmer::Function;

    pub fn exception_handler(store: &Store) -> Function {
        #[allow(unused)]
        fn func(a: i32) {}
        Function::new_native(store, func)
    }

    pub fn show_memory(store: &Store) -> Function {
        #[allow(unused)]
        fn func() {}
        Function::new_native(store, func)
    }

    pub fn print_error_message(store: &Store) -> Function {
        #[allow(unused)]
        fn func() {}
        Function::new_native(store, func)
    }

    pub fn write_buffer_message(store: &Store) -> Function {
        #[allow(unused)]
        fn func() {}
        Function::new_native(store, func)
    }
}

/// Read a base-{2^32} number given in little-endian format
fn from_array32<E: PairingEngine>(arr: Vec<u32>) -> E::Fr {
    let mut res = E::Fr::from(0 as u64);
    let mut current_multiple = E::Fr::from(1 as u64);
    let base = E::Fr::from(u32::MAX as u64 + 1);
    for val in arr {
        res += current_multiple * E::Fr::from(val as u64);
        current_multiple.mul_assign(&base);
    }
    res
}

/// Will return a little endian representation where each element of the array represent a 32-bit
/// chunk of the input
fn to_array32<E: PairingEngine>(s: &E::Fr, size: usize) -> Vec<u32> {
    let mut res = vec![0; size as usize];
    let bytes = s.into_repr().to_bytes_le();
    let l = bytes.len();
    let mut k = 0;
    for i in (0..l).step_by(4) {
        let mut chunk = [bytes[i]; 4];
        for j in 1..=3 {
            if i + j < l {
                chunk[j] = bytes[i + j];
            }
        }
        res[k] = u32::from_le_bytes(chunk);
        k += 1;
    }
    res
}

/// Check that the subgroup order is either for curve bn128 or bls12-381 and
/// the order should be the same as the curves of the pairing
pub(crate) fn check_subgroup_order<E: PairingEngine>(
    subgroup_order_bytes: &[u8],
) -> Result<Curve, CircomError> {
    let subgroup_order = BigUint::from_bytes_le(&subgroup_order_bytes);
    let subgroup_order_str = subgroup_order.to_string();

    let curve: Curve;
    if subgroup_order_str == BN128_ORDER {
        curve = Curve::Bn128;
    } else if subgroup_order_str == BLS12_381_ORDER {
        curve = Curve::Bls12_381;
    } else {
        return Err(CircomError::UnsupportedCurve(format!(
            "Unknown curve with order {:?}",
            subgroup_order_str
        )));
    }

    if subgroup_order.to_bytes_le() != <E::Fr as PrimeField>::Params::MODULUS.to_bytes_le() {
        return Err(CircomError::IncompatibleWithCurve);
    }
    Ok(curve)
}

fn fnv(inp: &str) -> (u32, u32) {
    let mut hasher = FnvHasher::default();
    hasher.write(inp.as_bytes());
    let h = hasher.finish();

    ((h >> 32) as u32, h as u32)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::circom;
    use ark_bls12_381::Bls12_381;
    use ark_bn254::Bn254;
    use num_bigint::BigInt;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::str::FromStr;

    fn big_int_to_ark_fr<E: PairingEngine>(big_int: BigInt) -> E::Fr {
        let (sign, mut abs) = big_int.into_parts();
        if sign == num_bigint::Sign::Minus {
            // Need to negate the witness element if negative
            let modulus = <<E::Fr as PrimeField>::Params as FpParameters>::MODULUS;
            abs = modulus.into() - abs;
        }
        E::Fr::from(abs)
    }

    struct TestCase<'a> {
        circuit_path: &'a str,
        inputs_path: &'a str,
        wires: &'a [&'a str],
    }

    #[test]
    fn multiplier_2_bn128() {
        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/multiply2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/multiply2_input1.json")
                .as_str(),
            wires: &["1", "33", "3", "11"],
        });

        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/multiply2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/multiply2_input2.json")
                .as_str(),
            wires: &[
                "1",
                "21888242871839275222246405745257275088548364400416034343698204186575672693159",
                "21888242871839275222246405745257275088548364400416034343698204186575796149939",
                "11",
            ],
        });

        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/multiply2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/multiply2_input3.json")
                .as_str(),
            wires: &[
                "1",
                "21888242871839275222246405745257275088548364400416034343698204186575808493616",
                "10944121435919637611123202872628637544274182200208017171849102093287904246808",
                "2",
            ],
        });
    }

    #[test]
    fn multiplier_2_bls12_381() {
        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/multiply2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/multiply2_input1.json")
                .as_str(),
            wires: &["1", "33", "3", "11"],
        });

        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/multiply2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/multiply2_input2.json")
                .as_str(),
            wires: &[
                "1",
                "19663453190672321429792902690569737189133957187697864183476372012476967944191",
                "6554484396890773809930967563523245729711319062565954727825457337492322648064",
                "11",
            ],
        });

        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/multiply2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/multiply2_input3.json")
                .as_str(),
            wires: &[
                "1",
                "26217937587563088935629642870386834316641451153972758222365847653897042132991",
                "39326906381344639707538691689286400077166001827250198022484753176917811658752",
                "2",
            ],
        });
    }

    #[test]
    fn test_1_bn128() {
        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/test1.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/test1_input1.json").as_str(),
            wires: &["1", "35", "3", "9"],
        });
        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/test1.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/test1_input2.json").as_str(),
            wires: &["1", "135", "5", "25"],
        });
    }

    #[test]
    fn test_1_bls12_381() {
        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/test1.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/test1_input1.json")
                .as_str(),
            wires: &["1", "35", "3", "9"],
        });
        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/test1.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/test1_input2.json")
                .as_str(),
            wires: &["1", "135", "5", "25"],
        });
    }

    #[test]
    fn test_2_bn128() {
        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/test2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/test2_input1.json").as_str(),
            wires: &["1", "12", "1", "2", "1", "4"],
        });
        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/test2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/test2_input2.json").as_str(),
            wires: &["1", "303", "4", "13", "16", "169"],
        });
    }

    #[test]
    fn test_2_bls12_381() {
        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/test2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/test2_input1.json")
                .as_str(),
            wires: &["1", "12", "1", "2", "1", "4"],
        });
        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/test2.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/test2_input2.json")
                .as_str(),
            wires: &["1", "303", "4", "13", "16", "169"],
        });
    }

    #[test]
    fn test_3_bn128() {
        run_test::<Bn254>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bn128/test3.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bn128/test3_input1.json").as_str(),
            wires: &[
                "1", "105165", "26050", "10", "25", "4", "5", "105", "1000", "40", "125", "1050",
            ],
        });
    }

    #[test]
    fn test_3_bls12_381() {
        run_test::<Bls12_381>(TestCase {
            circuit_path: circom::tests::abs_path("test-vectors/bls12-381/test3.wasm").as_str(),
            inputs_path: circom::tests::abs_path("test-vectors/bls12-381/test3_input1.json")
                .as_str(),
            wires: &[
                "1", "105165", "26050", "10", "25", "4", "5", "105", "1000", "40", "125", "1050",
            ],
        });
    }

    #[test]
    fn input_validation() {
        fn validate<E: PairingEngine>(circuit_path: &str) {
            let mut wtns = WitnessCalculator::<E>::from_wasm_file(circuit_path).unwrap();

            let err_1 = wtns
                .calculate_witnesses::<_>(
                    vec![("a".to_string(), vec![E::Fr::from(3u64)])].into_iter(),
                    false,
                )
                .unwrap_err();
            assert_eq!(err_1, CircomError::IncorrectNumberOfInputsProvided(2, 1));

            let err_2 = wtns
                .calculate_witnesses::<_>(
                    vec![("b".to_string(), vec![E::Fr::from(3u64)])].into_iter(),
                    false,
                )
                .unwrap_err();
            assert_eq!(err_2, CircomError::IncorrectNumberOfInputsProvided(2, 1));

            let err_3 = wtns
                .calculate_witnesses::<_>(
                    vec![("x".to_string(), vec![E::Fr::from(3u64)])].into_iter(),
                    false,
                )
                .unwrap_err();
            assert_eq!(
                err_3,
                CircomError::IncorrectNumberOfSignalsProvided("x".to_string(), 0, 1)
            );

            let err_4 = wtns
                .calculate_witnesses::<_>(
                    vec![
                        ("a".to_string(), vec![E::Fr::from(3u64)]),
                        ("b".to_string(), vec![E::Fr::from(10u64)]),
                        ("c".to_string(), vec![E::Fr::from(500u64)]),
                    ]
                    .into_iter(),
                    false,
                )
                .unwrap_err();
            assert_eq!(
                err_4,
                CircomError::IncorrectNumberOfSignalsProvided("c".to_string(), 0, 1)
            );

            let err_5 = wtns
                .calculate_witnesses::<_>(
                    vec![
                        ("a".to_string(), vec![]),
                        ("b".to_string(), vec![E::Fr::from(10u64)]),
                    ]
                    .into_iter(),
                    false,
                )
                .unwrap_err();
            assert_eq!(
                err_5,
                CircomError::IncorrectNumberOfSignalsProvided("a".to_string(), 1, 0)
            );

            let err_6 = wtns
                .calculate_witnesses::<_>(
                    vec![
                        ("a".to_string(), vec![E::Fr::from(3u64), E::Fr::from(5u64)]),
                        ("b".to_string(), vec![E::Fr::from(10u64)]),
                    ]
                    .into_iter(),
                    false,
                )
                .unwrap_err();
            assert_eq!(
                err_6,
                CircomError::IncorrectNumberOfSignalsProvided("a".to_string(), 1, 2)
            );

            assert!(wtns
                .calculate_witnesses::<_>(
                    vec![
                        ("a".to_string(), vec![E::Fr::from(5u64)]),
                        ("b".to_string(), vec![E::Fr::from(10u64)]),
                    ]
                    .into_iter(),
                    false
                )
                .is_ok());
        }

        validate::<Bn254>(circom::tests::abs_path("test-vectors/bn128/multiply2.wasm").as_str());
        validate::<Bls12_381>(
            circom::tests::abs_path("test-vectors/bls12-381/multiply2.wasm").as_str(),
        );

        assert_eq!(
            WitnessCalculator::<Bn254>::from_wasm_file(circom::tests::abs_path(
                "test-vectors/bls12-381/multiply2.wasm"
            ))
            .unwrap_err(),
            CircomError::IncompatibleWithCurve
        );
        assert_eq!(
            WitnessCalculator::<Bls12_381>::from_wasm_file(circom::tests::abs_path(
                "test-vectors/bn128/multiply2.wasm"
            ))
            .unwrap_err(),
            CircomError::IncompatibleWithCurve
        );

        assert_eq!(
            WitnessCalculator::<Bn254>::from_wasm_file(circom::tests::abs_path(
                "test-vectors/multiply2_goldilocks.wasm"
            ))
            .unwrap_err(),
            CircomError::UnsupportedCurve(
                "Unknown curve with order \"18446744069414584321\"".to_string()
            )
        );
        assert_eq!(
            WitnessCalculator::<Bls12_381>::from_wasm_file(circom::tests::abs_path(
                "test-vectors/multiply2_goldilocks.wasm"
            ))
            .unwrap_err(),
            CircomError::UnsupportedCurve(
                "Unknown curve with order \"18446744069414584321\"".to_string()
            )
        );
    }

    fn value_to_bigint(v: Value) -> BigInt {
        match v {
            Value::String(inner) => BigInt::from_str(&inner).unwrap(),
            Value::Number(inner) => BigInt::from(inner.as_u64().expect("not a u32")),
            _ => panic!("unsupported type"),
        }
    }

    fn run_test<E: PairingEngine>(case: TestCase) {
        let mut wtns = WitnessCalculator::<E>::from_wasm_file(case.circuit_path).unwrap();
        assert_eq!(
            wtns.instance.get_witness_count().unwrap(),
            case.wires.len() as u32
        );

        let inputs_str = std::fs::read_to_string(case.inputs_path).unwrap();
        let inputs: HashMap<String, serde_json::Value> = serde_json::from_str(&inputs_str).unwrap();

        let inputs = inputs
            .iter()
            .map(|(key, value)| {
                let res = match value {
                    Value::String(inner) => {
                        vec![BigInt::from_str(inner).unwrap()]
                    }
                    Value::Number(inner) => {
                        vec![BigInt::from(inner.as_u64().expect("not a u32"))]
                    }
                    Value::Array(inner) => inner.iter().cloned().map(value_to_bigint).collect(),
                    _ => panic!(),
                };

                (key.clone(), res)
            })
            .collect::<HashMap<_, _>>();

        assert_eq!(
            wtns.instance.get_input_count().unwrap(),
            inputs.len() as u32
        );

        let res = wtns
            .calculate_witnesses::<_>(
                inputs.clone().into_iter().map(|(n, v)| {
                    let f = v.into_iter().map(|b| big_int_to_ark_fr::<E>(b)).collect();
                    (n, f)
                }),
                false,
            )
            .unwrap();
        assert_eq!(res.len(), case.wires.len());
        for i in 0..res.len() {
            assert_eq!(
                res[i],
                big_int_to_ark_fr::<E>(BigInt::from_str(case.wires[i]).unwrap())
            );
        }
    }
}
