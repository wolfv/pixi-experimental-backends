//! Enumerate *all* build variants of a kernel as conda output records.
//!
//! The solver, not us, decides which variant is "best" for a given host: we
//! hand it every compatible variant (each with its `__cuda` / `__cuda_arch` /
//! `pytorch` constraints) and let unification pick the one whose constraints
//! the host virtual packages and environment satisfy. Variants we can't
//! express (cxx98, or an OS/arch with no conda subdir) are dropped with a reason.

use crate::mapping::{map_variant, CondaRecord, MapOptions};
use crate::variant::parse_variant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub variant: String,
    pub reason: String,
}

/// Return `(records, skipped)` for every variant directory of a kernel.
pub fn build_all_records(
    variant_names: &[String],
    opts: &MapOptions,
) -> (Vec<CondaRecord>, Vec<Skipped>) {
    let mut records = Vec::new();
    let mut skipped = Vec::new();

    for name in variant_names {
        match parse_variant(name) {
            Err(e) => skipped.push(Skipped {
                variant: name.clone(),
                reason: e.to_string(),
            }),
            Ok(variant) => match map_variant(&variant, opts) {
                Ok(rec) => records.push(rec),
                Err(e) => skipped.push(Skipped {
                    variant: name.clone(),
                    reason: e.to_string(),
                }),
            },
        }
    }
    (records, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn opts() -> MapOptions {
        MapOptions {
            package_name: "flash-attn".into(),
            version: "2.7.0".into(),
            build_number: 0,
            cuda_capabilities: vec![],
            require_cxx11: true,
        }
    }

    #[test]
    fn enumerates_and_skips() {
        let variants: Vec<String> = [
            "torch26-cxx98-cu118-x86_64-linux",  // skipped: cxx98
            "torch26-cxx11-cu118-x86_64-linux",  // ok
            "torch27-cxx11-cu126-aarch64-linux", // ok
            "torch27-cxx11-cu126-riscv64-linux", // skipped: no subdir
            "totally-broken",                    // skipped: malformed
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let (records, skipped) = build_all_records(&variants, &opts());

        let got: BTreeSet<_> = records.iter().map(|r| r.variant.as_str()).collect();
        assert_eq!(
            got,
            BTreeSet::from(["torch26-cxx11-cu118-x86_64-linux", "torch27-cxx11-cu126-aarch64-linux"])
        );
        assert_eq!(skipped.len(), 3);

        let subdirs: BTreeSet<_> = records.iter().map(|r| r.subdir.as_str()).collect();
        assert_eq!(subdirs, BTreeSet::from(["linux-64", "linux-aarch64"]));
    }
}
