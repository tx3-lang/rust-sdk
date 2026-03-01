//! Ergonomic facade for the full TX3 lifecycle.
//!
//! This module provides a high-level API that covers invocation, resolution,
//! signing, submission, and status polling.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::core::{ArgMap, BytesEnvelope};
use crate::tii::Protocol;
use crate::trp::{self, SubmitParams, TxStage, TxStatus, TxWitness};

#[derive(Clone)]
struct SignerParty {
    name: String,
    address: String,
    signer: Arc<dyn Signer + Send + Sync>,
}

/// Error type for facade operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Error originating from TII operations.
    #[error(transparent)]
    Tii(#[from] crate::tii::Error),

    /// Error originating from TRP operations.
    #[error(transparent)]
    Trp(#[from] crate::trp::Error),

    /// Required parameters were not provided.
    #[error("missing required params: {0:?}")]
    MissingParams(Vec<String>),

    /// A party was provided but not declared in the protocol.
    #[error("unknown party: {0}")]
    UnknownParty(String),

    /// Signer failed to produce a witness.
    #[error("signer error: {0}")]
    Signer(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Submitted hash does not match the resolved hash.
    #[error("submit hash mismatch: expected {expected}, got {received}")]
    SubmitHashMismatch { expected: String, received: String },

    /// Transaction failed to reach confirmation.
    #[error("tx {hash} failed with stage {stage:?}")]
    FinalizedFailed { hash: String, stage: TxStage },

    /// Transaction did not reach confirmation within the polling window.
    #[error("tx {hash} not confirmed after {attempts} attempts (delay {delay:?})")]
    FinalizedTimeout {
        hash: String,
        attempts: u32,
        delay: Duration,
    },
}

/// Configuration for check-status polling.
///
/// Used by `wait_for_confirmed` and `wait_for_finalized`.
#[derive(Debug, Clone)]
pub struct PollConfig {
    /// Number of attempts before timing out.
    pub attempts: u32,
    /// Delay between attempts.
    pub delay: Duration,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            attempts: 20,
            delay: Duration::from_secs(5),
        }
    }
}

/// A signer capable of producing TRP witnesses.
///
/// Signers are address-aware and must return the address they correspond to.
pub trait Signer: Send + Sync {
    /// Returns the address associated with this signer.
    fn address(&self) -> &str;

    /// Signs a transaction hash given as hex-encoded bytes.
    fn sign(&self, tx_hash: &str) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>>;
}

/// A party referenced by the protocol.
#[derive(Clone)]
pub enum Party {
    /// Read-only party with a known address.
    Address(String),
    /// Party capable of signing transactions.
    Signer {
        /// Party address (used for invocation args).
        address: String,
        /// Signer implementation.
        signer: Arc<dyn Signer + Send + Sync>,
    },
}

impl Party {
    /// Creates a read-only party from an address.
    pub fn address(address: impl Into<String>) -> Self {
        Party::Address(address.into())
    }

    /// Creates a signer party from a signer.
    ///
    /// The party address is taken from the signer itself.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tx3_sdk::{CardanoSigner, Party};
    ///
    /// let signer = CardanoSigner::from_hex("addr_test1...", "deadbeef...")?;
    /// let party = Party::signer(signer);
    /// # Ok::<(), tx3_sdk::Error>(())
    /// ```
    pub fn signer(signer: impl Signer + 'static) -> Self {
        Party::Signer {
            address: signer.address().to_string(),
            signer: Arc::new(signer),
        }
    }

    fn address_value(&self) -> &str {
        match self {
            Party::Address(address) => address,
            Party::Signer { address, .. } => address,
        }
    }

    fn signer_party(&self, name: &str) -> Option<SignerParty> {
        match self {
            Party::Signer { address, signer } => Some(SignerParty {
                name: name.to_string(),
                address: address.clone(),
                signer: Arc::clone(signer),
            }),
            _ => None,
        }
    }
}

/// High-level client that ties a protocol to a TRP client.
#[derive(Clone)]
pub struct Tx3Client {
    protocol: Arc<Protocol>,
    trp: trp::Client,
    parties: HashMap<String, Party>,
    profile: Option<String>,
}

impl Tx3Client {
    /// Creates a new facade client.
    pub fn new(protocol: Protocol, trp: trp::Client) -> Self {
        Self {
            protocol: Arc::new(protocol),
            trp,
            parties: HashMap::new(),
            profile: None,
        }
    }

    /// Sets the profile for all invocations created by this client.
    ///
    /// This profile is applied to every invocation created by the client.
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Attaches a party definition to this client.
    pub fn with_party(mut self, name: impl Into<String>, party: Party) -> Self {
        self.parties.insert(name.into().to_lowercase(), party);
        self
    }

    /// Attaches multiple party definitions to this client.
    pub fn with_parties<I, K>(mut self, parties: I) -> Self
    where
        I: IntoIterator<Item = (K, Party)>,
        K: Into<String>,
    {
        for (name, party) in parties {
            self.parties.insert(name.into().to_lowercase(), party);
        }
        self
    }

    /// Starts building a transaction invocation.
    pub fn tx(&self, name: impl Into<String>) -> TxBuilder {
        TxBuilder {
            protocol: Arc::clone(&self.protocol),
            trp: self.trp.clone(),
            tx_name: name.into(),
            args: ArgMap::new(),
            parties: self.parties.clone(),
            profile: self.profile.clone(),
        }
    }
}

/// Builder for transaction invocation.
pub struct TxBuilder {
    protocol: Arc<Protocol>,
    trp: trp::Client,
    tx_name: String,
    args: ArgMap,
    parties: HashMap<String, Party>,
    profile: Option<String>,
}

impl TxBuilder {
    /// Adds a single argument (case-insensitive name).
    pub fn arg(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.args.insert(name.to_lowercase(), value.into());
        self
    }

    /// Adds multiple arguments (case-insensitive names).
    pub fn args(mut self, args: ArgMap) -> Self {
        for (key, value) in args {
            self.args.insert(key.to_lowercase(), value);
        }
        self
    }

    /// Resolves the transaction using the TRP client.
    pub async fn resolve(self) -> Result<ResolvedTx, Error> {
        let mut invocation = self
            .protocol
            .invoke(&self.tx_name, self.profile.as_deref())?;

        let known_parties: HashSet<String> = self
            .protocol
            .parties()
            .keys()
            .map(|key| key.to_lowercase())
            .collect();

        for (name, party) in &self.parties {
            if !known_parties.contains(name) {
                return Err(Error::UnknownParty(name.clone()));
            }

            invocation.set_arg(
                name,
                serde_json::Value::String(party.address_value().to_string()),
            );
        }

        invocation.set_args(self.args);

        let mut missing: Vec<String> = invocation
            .unspecified_params()
            .map(|(key, _)| key.clone())
            .collect();

        if !missing.is_empty() {
            missing.sort();
            return Err(Error::MissingParams(missing));
        }

        let resolve_params = invocation.into_resolve_request()?;
        let envelope = self.trp.resolve(resolve_params).await?;

        let signers = self
            .parties
            .iter()
            .filter_map(|(name, party)| party.signer_party(name))
            .collect();

        Ok(ResolvedTx {
            trp: self.trp,
            hash: envelope.hash,
            tx_hex: envelope.tx,
            signers,
        })
    }
}

/// A resolved transaction ready for signing.
pub struct ResolvedTx {
    trp: trp::Client,
    /// Transaction hash.
    pub hash: String,
    /// Hex-encoded CBOR transaction bytes.
    pub tx_hex: String,
    signers: Vec<SignerParty>,
}

impl ResolvedTx {
    /// Returns the transaction hash that signers will sign.
    pub fn signing_hash(&self) -> &str {
        &self.hash
    }

    /// Signs the transaction with every signer party.
    pub fn sign(self) -> Result<SignedTx, Error> {
        let mut witnesses = Vec::with_capacity(self.signers.len());
        let mut witnesses_info = Vec::with_capacity(self.signers.len());

        for signer_party in &self.signers {
            let witness = signer_party
                .signer
                .sign(&self.hash)
                .map_err(Error::Signer)?;
            witnesses_info.push(WitnessInfo {
                party: signer_party.name.clone(),
                address: signer_party.address.clone(),
                key: witness.key.clone(),
                signature: witness.signature.clone(),
                witness_type: witness.witness_type.clone(),
                signed_hash: self.hash.clone(),
            });
            witnesses.push(witness);
        }

        let submit = SubmitParams {
            tx: BytesEnvelope {
                content: self.tx_hex,
                content_type: "hex".to_string(),
            },
            witnesses,
        };

        Ok(SignedTx {
            trp: self.trp,
            hash: self.hash,
            submit,
            witnesses_info,
        })
    }
}

/// Witness payloads for submission.
#[derive(Debug, Clone)]
pub struct WitnessInfo {
    /// Party name from the protocol.
    pub party: String,
    /// Party address used in invocation args.
    pub address: String,
    /// Public key envelope sent to the server.
    pub key: BytesEnvelope,
    /// Signature envelope sent to the server.
    pub signature: BytesEnvelope,
    /// Witness type.
    pub witness_type: trp::WitnessType,
    /// Transaction hash that was signed.
    pub signed_hash: String,
}

/// A signed transaction ready for submission.
pub struct SignedTx {
    trp: trp::Client,
    /// Resolved transaction hash.
    pub hash: String,
    /// Submit parameters including witnesses.
    pub submit: SubmitParams,
    witnesses_info: Vec<WitnessInfo>,
}

impl SignedTx {
    /// Returns witness payloads for submission.
    pub fn witnesses(&self) -> &[WitnessInfo] {
        &self.witnesses_info
    }
    /// Submits the signed transaction.
    pub async fn submit(self) -> Result<SubmittedTx, Error> {
        let response = self.trp.submit(self.submit).await?;

        if response.hash != self.hash {
            return Err(Error::SubmitHashMismatch {
                expected: self.hash,
                received: response.hash,
            });
        }

        Ok(SubmittedTx {
            trp: self.trp,
            hash: response.hash,
        })
    }
}

/// A submitted transaction that can be polled for status.
pub struct SubmittedTx {
    trp: trp::Client,
    /// Submitted transaction hash.
    pub hash: String,
}

impl SubmittedTx {
    /// Polls check-status until the transaction is confirmed or fails.
    pub async fn wait_for_confirmed(&self, config: PollConfig) -> Result<TxStatus, Error> {
        self.wait_for_stage(config, TxStage::Confirmed).await
    }

    /// Polls check-status until the transaction is finalized or fails.
    pub async fn wait_for_finalized(&self, config: PollConfig) -> Result<TxStatus, Error> {
        self.wait_for_stage(config, TxStage::Finalized).await
    }

    async fn wait_for_stage(&self, config: PollConfig, target: TxStage) -> Result<TxStatus, Error> {
        for attempt in 1..=config.attempts {
            let response = self.trp.check_status(vec![self.hash.clone()]).await?;

            if let Some(status) = response.statuses.get(&self.hash) {
                match status.stage {
                    TxStage::Finalized => return Ok(status.clone()),
                    TxStage::Confirmed if matches!(target, TxStage::Confirmed) => {
                        return Ok(status.clone())
                    }
                    TxStage::Dropped | TxStage::RolledBack => {
                        return Err(Error::FinalizedFailed {
                            hash: self.hash.clone(),
                            stage: status.stage.clone(),
                        });
                    }
                    _ => {}
                }
            }

            if attempt < config.attempts {
                tokio::time::sleep(config.delay).await;
            }
        }

        Err(Error::FinalizedTimeout {
            hash: self.hash.clone(),
            attempts: config.attempts,
            delay: config.delay,
        })
    }
}

/// Signer implementations.
pub mod signer {
    use super::Signer;
    use crate::core::BytesEnvelope;
    use crate::trp::{TxWitness, WitnessType};
    use cryptoxide::hmac::Hmac;
    use cryptoxide::pbkdf2::pbkdf2;
    use cryptoxide::sha2::Sha512;
    use ed25519_bip32::{DerivationScheme, XPrv, XPRV_SIZE};
    use pallas_addresses::{Address, ShelleyPaymentPart};
    use pallas_crypto::hash::Hasher;
    use pallas_crypto::key::ed25519::{SecretKey, SecretKeyExtended, Signature};
    use thiserror::Error;

    /// Errors returned by the built-in ed25519 signer.
    #[derive(Debug, Error)]
    pub enum SignerError {
        /// Mnemonic phrase could not be parsed.
        #[error("invalid mnemonic: {0}")]
        InvalidMnemonic(bip39::Error),

        /// Private key hex could not be decoded.
        #[error("invalid private key hex: {0}")]
        InvalidPrivateKeyHex(hex::FromHexError),

        /// Private key length is not 32 bytes.
        #[error("private key must be 32 bytes, got {0}")]
        InvalidPrivateKeyLength(usize),

        /// Transaction hash hex could not be decoded.
        #[error("invalid tx hash hex: {0}")]
        InvalidHashHex(hex::FromHexError),

        /// Transaction hash length is not 32 bytes.
        #[error("transaction hash must be 32 bytes, got {0}")]
        InvalidHashLength(usize),

        /// Address could not be parsed.
        #[error("invalid address: {0}")]
        InvalidAddress(pallas_addresses::Error),

        /// Address does not contain a payment key hash.
        #[error("address does not contain a payment key hash")]
        UnsupportedPaymentCredential,

        /// Signer key doesn't match address payment key.
        #[error("signer key doesn't match address payment key")]
        AddressMismatch,
    }

    /// Built-in ed25519 signer using a 32-byte private key.
    ///
    /// The address is required at construction and returned via `Signer::address`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tx3_sdk::Ed25519Signer;
    ///
    /// let signer = Ed25519Signer::from_hex("addr_test1...", "deadbeef...")?;
    /// # Ok::<(), tx3_sdk::Error>(())
    /// ```
    #[derive(Debug, Clone)]
    pub struct Ed25519Signer {
        address: String,
        private_key: [u8; 32],
    }

    impl Ed25519Signer {
        /// Creates a signer from a raw 32-byte private key and address.
        pub fn new(address: impl Into<String>, private_key: [u8; 32]) -> Self {
            Self {
                address: address.into(),
                private_key,
            }
        }

        /// Creates a signer from a BIP39 mnemonic phrase.
        ///
        /// The address is required and stored on the signer.
        pub fn from_mnemonic(
            address: impl Into<String>,
            phrase: &str,
        ) -> Result<Self, SignerError> {
            let mnemonic = bip39::Mnemonic::parse(phrase).map_err(SignerError::InvalidMnemonic)?;
            let seed = mnemonic.to_seed("");

            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&seed[0..32]);

            Ok(Self::new(address, key_array))
        }

        /// Creates a signer from a hex-encoded 32-byte private key.
        ///
        /// The address is required and stored on the signer.
        pub fn from_hex(
            address: impl Into<String>,
            private_key_hex: &str,
        ) -> Result<Self, SignerError> {
            let key_bytes =
                hex::decode(private_key_hex).map_err(SignerError::InvalidPrivateKeyHex)?;

            if key_bytes.len() != 32 {
                return Err(SignerError::InvalidPrivateKeyLength(key_bytes.len()));
            }

            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&key_bytes);

            Ok(Self::new(address, key_array))
        }
    }

    /// Cardano signer that derives witness key from address payment part.
    ///
    /// This signer derives keys using the Cardano path `m/1852'/1815'/0'/0/0`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tx3_sdk::CardanoSigner;
    ///
    /// let signer = CardanoSigner::from_mnemonic(
    ///     "addr_test1...",
    ///     "word1 word2 ... word24",
    /// )?;
    /// # Ok::<(), tx3_sdk::Error>(())
    /// ```
    #[derive(Debug, Clone)]
    pub struct CardanoSigner {
        address: String,
        private_key: CardanoPrivateKey,
        payment_key_hash: Vec<u8>,
    }

    #[derive(Debug, Clone)]
    enum CardanoPrivateKey {
        Normal(SecretKey),
        Extended(SecretKeyExtended),
    }

    impl CardanoPrivateKey {
        fn public_key_bytes(&self) -> Vec<u8> {
            match self {
                CardanoPrivateKey::Normal(key) => key.public_key().as_ref().to_vec(),
                CardanoPrivateKey::Extended(key) => key.public_key().as_ref().to_vec(),
            }
        }

        fn sign(&self, msg: &[u8]) -> Signature {
            match self {
                CardanoPrivateKey::Normal(key) => key.sign(msg),
                CardanoPrivateKey::Extended(key) => key.sign(msg),
            }
        }
    }

    impl CardanoSigner {
        /// Creates a Cardano signer from a raw private key and address.
        fn new(
            private_key: CardanoPrivateKey,
            address: impl Into<String>,
        ) -> Result<Self, SignerError> {
            let address = address.into();
            let payment_key_hash = extract_payment_key_hash(&address)?;
            Ok(Self {
                address,
                private_key,
                payment_key_hash,
            })
        }

        /// Creates a Cardano signer from a hex-encoded private key and address.
        pub fn from_hex(
            address: impl Into<String>,
            private_key_hex: &str,
        ) -> Result<Self, SignerError> {
            let key_bytes =
                hex::decode(private_key_hex).map_err(SignerError::InvalidPrivateKeyHex)?;

            if key_bytes.len() != 32 {
                return Err(SignerError::InvalidPrivateKeyLength(key_bytes.len()));
            }

            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&key_bytes);

            let key: SecretKey = key_array.into();

            Self::new(CardanoPrivateKey::Normal(key), address)
        }

        /// Creates a Cardano signer from a mnemonic phrase and address.
        pub fn from_mnemonic(
            address: impl Into<String>,
            phrase: &str,
        ) -> Result<Self, SignerError> {
            let root = derive_root_xprv(phrase, "")?;
            let payment = derive_cardano_payment_xprv(&root);
            let key =
                unsafe { SecretKeyExtended::from_bytes_unchecked(payment.extended_secret_key()) };

            Self::new(CardanoPrivateKey::Extended(key), address)
        }

        fn verify_address_binding(&self, public_key_bytes: &[u8]) -> Result<(), SignerError> {
            let mut hasher = Hasher::<224>::new();
            hasher.input(public_key_bytes);
            let digest = hasher.finalize();

            if digest.as_ref() != self.payment_key_hash.as_slice() {
                return Err(SignerError::AddressMismatch);
            }

            Ok(())
        }
    }

    impl Signer for CardanoSigner {
        fn address(&self) -> &str {
            &self.address
        }

        fn sign(
            &self,
            tx_hash: &str,
        ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>> {
            let hash_bytes = hex::decode(tx_hash).map_err(|err| {
                Box::new(SignerError::InvalidHashHex(err))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            if hash_bytes.len() != 32 {
                return Err(Box::new(SignerError::InvalidHashLength(hash_bytes.len())));
            }

            let public_key_bytes = self.private_key.public_key_bytes();

            let _ = self.verify_address_binding(&public_key_bytes);

            let signature = self.private_key.sign(&hash_bytes);

            Ok(TxWitness {
                key: BytesEnvelope {
                    content: hex::encode(&public_key_bytes),
                    content_type: "hex".to_string(),
                },
                signature: BytesEnvelope {
                    content: hex::encode(signature.as_ref()),
                    content_type: "hex".to_string(),
                },
                witness_type: WitnessType::VKey,
            })
        }
    }

    fn derive_root_xprv(phrase: &str, password: &str) -> Result<XPrv, SignerError> {
        let mnemonic = bip39::Mnemonic::parse(phrase).map_err(SignerError::InvalidMnemonic)?;
        let entropy = mnemonic.to_entropy();

        let mut pbkdf2_result = [0u8; XPRV_SIZE];

        const ITER: u32 = 4096;

        let mut mac = Hmac::new(Sha512::new(), password.as_bytes());
        pbkdf2(&mut mac, &entropy, ITER, &mut pbkdf2_result);

        Ok(XPrv::normalize_bytes_force3rd(pbkdf2_result))
    }

    fn derive_cardano_payment_xprv(root: &XPrv) -> XPrv {
        const HARDENED: u32 = 0x8000_0000;

        root.derive(DerivationScheme::V2, 1852 | HARDENED)
            .derive(DerivationScheme::V2, 1815 | HARDENED)
            .derive(DerivationScheme::V2, 0 | HARDENED)
            .derive(DerivationScheme::V2, 0)
            .derive(DerivationScheme::V2, 0)
    }

    fn extract_payment_key_hash(address: &str) -> Result<Vec<u8>, SignerError> {
        let parsed = Address::from_bech32(address).map_err(SignerError::InvalidAddress)?;

        let payment = match parsed {
            Address::Shelley(addr) => addr.payment().clone(),
            _ => return Err(SignerError::UnsupportedPaymentCredential),
        };

        match payment {
            ShelleyPaymentPart::Key(hash) => Ok(hash.as_ref().to_vec()),
            ShelleyPaymentPart::Script(_) => Err(SignerError::UnsupportedPaymentCredential),
        }
    }

    impl Signer for Ed25519Signer {
        fn address(&self) -> &str {
            &self.address
        }

        fn sign(
            &self,
            tx_hash: &str,
        ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>> {
            let hash_bytes = hex::decode(tx_hash).map_err(|err| {
                Box::new(SignerError::InvalidHashHex(err))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;

            if hash_bytes.len() != 32 {
                return Err(Box::new(SignerError::InvalidHashLength(hash_bytes.len())));
            }

            let signing_key: SecretKey = self.private_key.into();
            let public_key = signing_key.public_key();
            let signature = signing_key.sign(&hash_bytes);

            Ok(TxWitness {
                key: BytesEnvelope {
                    content: hex::encode(public_key.as_ref()),
                    content_type: "hex".to_string(),
                },
                signature: BytesEnvelope {
                    content: hex::encode(signature.as_ref()),
                    content_type: "hex".to_string(),
                },
                witness_type: WitnessType::VKey,
            })
        }
    }
}
