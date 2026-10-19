//! Hashing functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue};

pub struct Fnv1a64;

impl FfiFunction for Fnv1a64 {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        let bytes = match &args[0] {
            FfiValue::String(s) => s.as_bytes(),
            _ => {
                return Err(FfiError::ArgumentType {
                    expected: "String".to_string(),
                    got: args[0].type_name().to_string(),
                })
            }
        };

        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;
        for &byte in bytes {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        Ok(FfiValue::Number((hash as i64) as f64))
    }

    fn name(&self) -> &str {
        "Fnv1a64"
    }

    fn arity(&self) -> usize {
        1
    }

    fn description(&self) -> Option<&str> {
        Some("64-bit FNV-1a non-cryptographic hash of string")
    }
}

pub struct Crc32;

impl FfiFunction for Crc32 {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        let bytes = match &args[0] {
            FfiValue::String(s) => s.as_bytes(),
            _ => {
                return Err(FfiError::ArgumentType {
                    expected: "String".to_string(),
                    got: args[0].type_name().to_string(),
                })
            }
        };

        // Standard IEEE 802.3 CRC32 lookup / bitwise algorithm
        let mut crc: u32 = 0xFFFFFFFF;
        for &byte in bytes {
            crc ^= byte as u32;
            for _ in 0..8 {
                let mask = (-( (crc & 1) as i32 )) as u32;
                crc = (crc >> 1) ^ (0xEDB88320 & mask);
            }
        }
        let final_crc = !crc;
        Ok(FfiValue::Number(final_crc as f64))
    }

    fn name(&self) -> &str {
        "Crc32"
    }

    fn arity(&self) -> usize {
        1
    }

    fn description(&self) -> Option<&str> {
        Some("Compute CRC-32 (IEEE 802.3) checksum of string")
    }
}

pub fn register_hash_functions(registry: &mut FfiRegistry) {
    registry.register(Fnv1a64);
    registry.register(Crc32);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContext;
    impl crate::FfiContext for MockContext {
        fn get_this(&self) -> Option<FfiValue> {
            None
        }
    }

    #[test]
    fn test_hash_functions() {
        let mut registry = FfiRegistry::new();
        register_hash_functions(&mut registry);
        let mut ctx = MockContext;

        let res = registry
            .call(&mut ctx, "Crc32", vec![FfiValue::String("123456789".to_string())])
            .unwrap();
        // Standard check: CRC-32 of "123456789" is 0xCBF43926 = 3421780262
        assert_eq!(res, FfiValue::Number(3421780262.0));

        let res = registry
            .call(&mut ctx, "Fnv1a64", vec![FfiValue::String("hello".to_string())])
            .unwrap();
        assert!(matches!(res, FfiValue::Number(_)));
    }
}
