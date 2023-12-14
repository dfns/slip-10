//! SLIP-10: Deterministic key generation
//!
//! [SLIP10] is a specification for implementing HD wallets. It aims at supporting many
//! curves while being compatible with [BIP32].
//!
//! The implementation is based on [generic-ec](generic_ec) library that provides generic
//! elliptic curve arithmetic. The crate is `no_std` and `no_alloc` friendly.
//!
//! ### Curves support
//! Implementation currently does not support ed25519 curve. All other curves are
//! supported: both secp256k1 and secp256r1. In fact, implementation may work with any
//! curve, but only those are covered by the SLIP10 specs.
//!
//! The crate also re-exports supported curves in [supported_curves] module (requires
//! enabling a feature), but any other curve implementation will work with the crate.
//!
//! ### Features
//! * `std`: enables std library support (mainly, it just implements [`Error`](std::error::Error)
//!   trait for the error types)
//! * `curve-secp256k1` and `curve-secp256r1` add curve implementation into the crate [supported_curves]
//!   module
//!
//! ### Examples
//!
//! Derive a master key from the seed, and then derive a child key m/1<sub>H</sub>/10:
//! ```rust
//! use slip10::supported_curves::Secp256k1;
//!
//! let seed = b"16-64 bytes of high entropy".as_slice();
//! let master_key = slip10::derive_master_key::<Secp256k1>(
//!     slip10::CurveType::Secp256k1,
//!     seed,
//! )?;
//! let master_key_pair = slip10::ExtendedKeyPair::from(master_key);
//!
//! let derivation_path = [1 + slip10::H, 10];
//! let mut derived_key = master_key_pair;
//! for child_index in derivation_path {
//!     derived_key = slip10::derive_child_key_pair(
//!         &derived_key,
//!         child_index,
//!     );
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [SLIP10]: https://github.com/satoshilabs/slips/blob/master/slip-0010.md
//! [BIP32]: https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(missing_docs, unsafe_code)]

use core::ops;

use generic_ec::{Curve, Point, Scalar, SecretScalar};
use hmac::Mac as _;

#[cfg(any(
    feature = "curve-secp256k1",
    feature = "curve-secp256r1",
    feature = "all-curves"
))]
pub use generic_ec::curves as supported_curves;

pub mod errors;

type HmacSha512 = hmac::Hmac<sha2::Sha512>;
/// Beggining of hardened child indexes
///
/// $H = 2^{31}$ defines the range of hardened indexes. All indexes $i$ such that $H \le i$ are hardened.
///
/// ## Example
/// Derive a child key with a path m/1<sub>H</sub>
/// ```rust
/// use slip10::supported_curves::Secp256k1;
///
/// # let seed = b"do not use this seed in prod :)".as_slice();
/// let master_key = slip10::derive_master_key::<Secp256k1>(
///     slip10::CurveType::Secp256k1,
///     seed,
/// )?;
/// let master_key_pair = slip10::ExtendedKeyPair::from(master_key);
///
/// let hardened_child = slip10::derive_child_key_pair(
///     &master_key_pair,
///     1 + slip10::H,
/// );
/// #
/// # Ok::<(), slip10::errors::InvalidLength>(())
/// ```
pub const H: u32 = 1 << 31;

/// Child index, whether hardened or not
#[derive(Clone, Copy, Debug)]
pub enum ChildIndex {
    /// Hardened index
    Hardened(HardenedIndex),
    /// Non-hardened index
    NonHardened(NonHardenedIndex),
}

/// Child index in range $2^{31} \le i < 2^{32}$ corresponing to a hardened wallet
#[derive(Clone, Copy, Debug)]
pub struct HardenedIndex(u32);

/// Child index in range $0 \le i < 2^{31}$ corresponing to a non-hardened wallet
#[derive(Clone, Copy, Debug)]
pub struct NonHardenedIndex(u32);

/// Extended public key
#[derive(Clone, Copy, Debug)]
pub struct ExtendedPublicKey<E: Curve> {
    /// The public key that can be used for signature verification
    pub public_key: Point<E>,
    /// A chain code that is used to derive child keys
    pub chain_code: ChainCode,
}

/// Extended secret key
#[derive(Clone, Debug)]
pub struct ExtendedSecretKey<E: Curve> {
    /// The secret key that can be used for signing
    pub secret_key: SecretScalar<E>,
    /// A chain code that is used to derive child keys
    pub chain_code: ChainCode,
}

/// Pair of extended secret and public keys
#[derive(Clone, Debug)]
pub struct ExtendedKeyPair<E: Curve> {
    public_key: ExtendedPublicKey<E>,
    secret_key: ExtendedSecretKey<E>,
}

/// A shift that can be applied to parent key to obtain a child key
///
/// It contains an already derived child public key as it needs to be derived
/// in process of calculating the shift value
#[derive(Clone, Copy, Debug)]
pub struct DerivedShift<E: Curve> {
    /// Derived shift
    pub shift: Scalar<E>,
    /// Derived child extended public key
    pub child_public_key: ExtendedPublicKey<E>,
}

/// Chain code of extended key as defined in SLIP-10
pub type ChainCode = [u8; 32];

impl HardenedIndex {
    /// The smallest possible value of hardened index. Equals to $2^{31}$
    pub const MIN: Self = Self(H);
    /// The largest possible value of hardened index. Equals to $2^{32} - 1$
    pub const MAX: Self = Self(u32::MAX);
}
impl NonHardenedIndex {
    /// The smallest possible value of non-hardened index. Equals to $0$
    pub const MIN: Self = Self(0);
    /// The largest possible value of non-hardened index. Equals to $2^{31} - 1$
    pub const MAX: Self = Self(H - 1);
}
impl ops::Deref for HardenedIndex {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl ops::Deref for NonHardenedIndex {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl ops::Deref for ChildIndex {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Hardened(i) => &*i,
            Self::NonHardened(i) => &*i,
        }
    }
}
impl From<u32> for ChildIndex {
    fn from(value: u32) -> Self {
        match value {
            H.. => Self::Hardened(HardenedIndex(value)),
            _ => Self::NonHardened(NonHardenedIndex(value)),
        }
    }
}
impl TryFrom<u32> for HardenedIndex {
    type Error = errors::OutOfRange;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match ChildIndex::from(value) {
            ChildIndex::Hardened(v) => Ok(v),
            _ => Err(errors::OutOfRange),
        }
    }
}
impl TryFrom<u32> for NonHardenedIndex {
    type Error = errors::OutOfRange;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match ChildIndex::from(value) {
            ChildIndex::NonHardened(v) => Ok(v),
            _ => Err(errors::OutOfRange),
        }
    }
}

impl<E: Curve> From<&ExtendedSecretKey<E>> for ExtendedPublicKey<E> {
    fn from(sk: &ExtendedSecretKey<E>) -> Self {
        ExtendedPublicKey {
            public_key: Point::generator() * &sk.secret_key,
            chain_code: sk.chain_code,
        }
    }
}

impl<E: Curve> From<ExtendedSecretKey<E>> for ExtendedKeyPair<E> {
    fn from(secret_key: ExtendedSecretKey<E>) -> Self {
        Self {
            public_key: (&secret_key).into(),
            secret_key,
        }
    }
}

impl<E: Curve> ExtendedKeyPair<E> {
    /// Returns chain code of the key
    pub fn chain_code(&self) -> &ChainCode {
        debug_assert_eq!(self.public_key.chain_code, self.secret_key.chain_code);
        &self.public_key.chain_code
    }

    /// Returns extended public key
    pub fn public_key(&self) -> &ExtendedPublicKey<E> {
        &self.public_key
    }

    /// Returns extended secret key
    pub fn secret_key(&self) -> &ExtendedSecretKey<E> {
        &self.secret_key
    }
}

/// Curves supported by SLIP-10 spec
///
/// It's either secp256k1 or secp256r1. Note that SLIP-10 also supports ed25519 curve, but this library
/// does not support it.
///
/// `CurveType` is only needed for master key derivation.
#[derive(Clone, Copy, Debug)]
pub enum CurveType {
    /// Secp256k1 curve
    Secp256k1,
    /// Secp256r1 curve
    Secp256r1,
}

/// Derives a master key from the seed
///
/// Seed must be 16-64 bytes long, otherwise an error is returned
pub fn derive_master_key<E: Curve>(
    curve_type: CurveType,
    seed: &[u8],
) -> Result<ExtendedSecretKey<E>, errors::InvalidLength> {
    if !(16 <= seed.len() && seed.len() <= 64) {
        return Err(errors::InvalidLength);
    }

    let curve = match curve_type {
        CurveType::Secp256k1 => "Bitcoin seed",
        CurveType::Secp256r1 => "Nist256p1 seed",
    };

    let hmac = HmacSha512::new_from_slice(curve.as_bytes())
        .expect("this never fails: hmac can handle keys of any size");
    let mut i = hmac.clone().chain_update(seed).finalize().into_bytes();

    loop {
        let i_left = &i[..32];
        let i_right: [u8; 32] = i[32..]
            .try_into()
            .expect("this should never fail as size of output is fixed");

        if let Ok(mut sk) = Scalar::<E>::from_be_bytes(i_left) {
            if !bool::from(subtle::ConstantTimeEq::ct_eq(&sk, &Scalar::zero())) {
                return Ok(ExtendedSecretKey {
                    secret_key: SecretScalar::new(&mut sk),
                    chain_code: i_right,
                });
            }
        }

        i = hmac.clone().chain_update(&i[..]).finalize().into_bytes()
    }
}

/// Derives child key pair (extended secret key + public key) from parent key pair
///
/// ### Example
/// Derive child key m/1<sub>H</sub> from master key
/// ```rust
/// use slip10::supported_curves::Secp256k1;
///
/// # let seed = b"do not use this seed :)".as_slice();
/// let master_key = slip10::derive_master_key::<Secp256k1>(
///     slip10::CurveType::Secp256k1,
///     seed,
/// )?;
/// let master_key_pair = slip10::ExtendedKeyPair::from(master_key);
///
/// let derived_key = slip10::derive_child_key_pair(
///     &master_key_pair,
///     1 + slip10::H,
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn derive_child_key_pair<E: Curve>(
    parent_key: &ExtendedKeyPair<E>,
    child_index: impl Into<ChildIndex>,
) -> ExtendedKeyPair<E> {
    let child_index = child_index.into();
    let shift = match child_index {
        ChildIndex::Hardened(i) => derive_hardened_shift(parent_key, i),
        ChildIndex::NonHardened(i) => derive_public_shift(&parent_key.public_key, i),
    };
    let mut child_sk = &parent_key.secret_key.secret_key + shift.shift;
    let child_sk = SecretScalar::new(&mut child_sk);
    ExtendedKeyPair {
        secret_key: ExtendedSecretKey {
            secret_key: child_sk,
            chain_code: shift.child_public_key.chain_code,
        },
        public_key: shift.child_public_key,
    }
}

/// Derives child extended public key from parent extended public key
///
/// ### Example
/// Derive a master public key m/1
/// ```rust
/// use slip10::supported_curves::Secp256k1;
///
/// # let seed = b"do not use this seed :)".as_slice();
/// let master_key = slip10::derive_master_key::<Secp256k1>(
///     slip10::CurveType::Secp256k1,
///     seed,
/// )?;
/// let master_public_key = slip10::ExtendedPublicKey::from(&master_key);
///
/// let derived_key = slip10::derive_child_public_key(
///     &master_public_key,
///     1.try_into()?,
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn derive_child_public_key<E: Curve>(
    parent_public_key: &ExtendedPublicKey<E>,
    child_index: NonHardenedIndex,
) -> ExtendedPublicKey<E> {
    derive_public_shift(parent_public_key, child_index).child_public_key
}

/// Derive a shift for hardened child
pub fn derive_hardened_shift<E: Curve>(
    parent_key: &ExtendedKeyPair<E>,
    child_index: HardenedIndex,
) -> DerivedShift<E> {
    let hmac = HmacSha512::new_from_slice(parent_key.chain_code())
        .expect("this never fails: hmac can handle keys of any size");
    let i = hmac
        .clone()
        .chain_update([0x00])
        .chain_update(parent_key.secret_key.secret_key.as_ref().to_be_bytes())
        .chain_update(child_index.to_be_bytes())
        .finalize()
        .into_bytes();
    calculate_shift(&hmac, &parent_key.public_key, *child_index, i)
}

/// Derives a shift for non-hardened child
pub fn derive_public_shift<E: Curve>(
    parent_public_key: &ExtendedPublicKey<E>,
    child_index: NonHardenedIndex,
) -> DerivedShift<E> {
    let hmac = HmacSha512::new_from_slice(&parent_public_key.chain_code)
        .expect("this never fails: hmac can handle keys of any size");
    let i = hmac
        .clone()
        .chain_update(&parent_public_key.public_key.to_bytes(true))
        .chain_update(child_index.to_be_bytes())
        .finalize()
        .into_bytes();
    calculate_shift(&hmac, parent_public_key, *child_index, i)
}

fn calculate_shift<E: Curve>(
    hmac: &HmacSha512,
    parent_public_key: &ExtendedPublicKey<E>,
    child_index: u32,
    mut i: hmac::digest::Output<HmacSha512>,
) -> DerivedShift<E> {
    loop {
        let i_left = &i[..32];
        let i_right: [u8; 32] = i[32..]
            .try_into()
            .expect("this should never fail as size of output is fixed");

        if let Ok(shift) = Scalar::<E>::from_be_bytes(i_left) {
            let child_pk = parent_public_key.public_key + Point::generator() * &shift;
            if !child_pk.is_zero() {
                return DerivedShift {
                    shift,
                    child_public_key: ExtendedPublicKey {
                        public_key: child_pk,
                        chain_code: i_right,
                    },
                };
            }
        }

        i = hmac
            .clone()
            .chain_update([0x01])
            .chain_update(i_right)
            .chain_update(child_index.to_be_bytes())
            .finalize()
            .into_bytes()
    }
}