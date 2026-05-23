//! Ergonomic facade for the full TX3 lifecycle.
//!
//! This module provides a high-level API that covers invocation, resolution,
//! signing, submission, and status polling.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::core::{ArgMap, BytesEnvelope, EnvMap, TirEnvelope};
use crate::tii::Protocol;
use crate::trp::{self, ResolveParams, SubmitParams, TxStage, TxStatus, TxWitness};

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

    /// A transaction name was not declared by the protocol.
    #[error("unknown transaction: {0}")]
    UnknownTx(String),

    /// A profile name was not declared by the protocol.
    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    /// A party name was not declared by the protocol.
    #[error("unknown party: {0}")]
    UnknownParty(String),

    /// The builder was finalized without a TRP endpoint.
    #[error("TRP endpoint not configured")]
    MissingTrpEndpoint,

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

/// Inputs passed to a [`Signer`] for each sign call.
///
/// Carries both the bound tx hash and the full hex-encoded tx CBOR. Hash-based
/// signers (Cardano, Ed25519) read `tx_hash_hex`; tx-based signers (e.g. wallet
/// adapters that need the full tx body) read `tx_cbor_hex`. The SDK always
/// populates both fields.
#[derive(Debug, Clone)]
pub struct SignRequest {
    /// Hex-encoded tx hash bound to this signing call.
    pub tx_hash_hex: String,
    /// Hex-encoded full tx CBOR.
    pub tx_cbor_hex: String,
}

/// A signer capable of producing TRP witnesses.
///
/// Signers are address-aware and must return the address they correspond to.
pub trait Signer: Send + Sync {
    /// Returns the address associated with this signer.
    fn address(&self) -> &str;

    /// Signs the transaction described by `request`.
    fn sign(
        &self,
        request: &SignRequest,
    ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>>;
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

/// A named profile baked into a client: environment values and party
/// addresses keyed by name.
///
/// Produced either by deconstructing a loaded [`Protocol`] inside
/// [`Tx3ClientBuilder::from_protocol`] or by parsing the JSON a generated
/// codegen client embeds (via [`Profile::load_all`]).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Profile {
    /// Environment values applied to every transaction under this profile.
    #[serde(default)]
    pub environment: EnvMap,
    /// Party addresses applied to every transaction under this profile.
    #[serde(default)]
    pub parties: HashMap<String, String>,
}

impl Profile {
    /// Parses a JSON map of profiles in the shape the codegen template embeds
    /// (`{ "profileName": { "environment": { ... }, "parties": { ... } }, ... }`).
    ///
    /// # Panics
    ///
    /// Panics if the JSON does not parse — codegen produces a valid map by
    /// construction, so a panic indicates a bug in the template.
    pub fn load_all(json: &str) -> HashMap<String, Profile> {
        serde_json::from_str(json).expect("codegen: invalid embedded profiles")
    }
}

/// High-level client over a TX3 protocol.
///
/// Holds the deconstructed protocol parts — per-transaction TIR envelopes,
/// named profiles, the set of declared party names — plus the runtime state
/// (TRP client, bound parties, selected profile and env overrides).
///
/// Construct one through [`Tx3ClientBuilder`], obtained via
/// [`Protocol::client`]: profile selection and party/env binding happen on
/// the builder, and `build()` performs all fallible validation.
#[derive(Clone)]
pub struct Tx3Client {
    transactions: HashMap<String, TirEnvelope>,
    known_parties: HashSet<String>,
    trp: trp::Client,
    bound_parties: HashMap<String, Party>,
    selected_profile: Option<Profile>,
    env_overrides: EnvMap,
}

impl Tx3Client {
    /// Constructs a client from already-deconstructed protocol parts.
    ///
    /// Crate-internal entry used by [`Tx3ClientBuilder::build`]. External
    /// callers go through the builder.
    pub(crate) fn from_parts(
        transactions: HashMap<String, TirEnvelope>,
        known_parties: HashSet<String>,
        trp: trp::Client,
        bound_parties: HashMap<String, Party>,
        selected_profile: Option<Profile>,
        env_overrides: EnvMap,
    ) -> Self {
        let known_parties = known_parties
            .into_iter()
            .map(|name| name.to_lowercase())
            .collect();
        Self {
            transactions,
            known_parties,
            trp,
            bound_parties,
            selected_profile,
            env_overrides,
        }
    }

    /// Binds a party (signer or read-only address) by name after the client
    /// has been built. Useful for late binding when, e.g., a user logs in
    /// after the client is already in scope.
    ///
    /// Overrides any address the selected profile declared for the same name.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownParty`] if `name` is not a party declared by
    /// the protocol.
    pub fn with_party(
        mut self,
        name: impl Into<String>,
        party: Party,
    ) -> Result<Self, Error> {
        let name = name.into().to_lowercase();
        if !self.known_parties.contains(&name) {
            return Err(Error::UnknownParty(name));
        }
        self.bound_parties.insert(name, party);
        Ok(self)
    }

    /// Binds a party without validating the name against the protocol's
    /// declared parties. Intended for codegen-generated wrappers — see
    /// [`Tx3ClientBuilder::with_party_unchecked`]. Hand-written code SHOULD
    /// use [`Tx3Client::with_party`].
    pub fn with_party_unchecked(
        mut self,
        name: impl Into<String>,
        party: Party,
    ) -> Self {
        self.bound_parties
            .insert(name.into().to_lowercase(), party);
        self
    }

    /// Binds multiple parties at once. See [`Tx3Client::with_party`].
    pub fn with_parties<I, K>(mut self, parties: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = (K, Party)>,
        K: Into<String>,
    {
        for (name, party) in parties {
            self = self.with_party(name, party)?;
        }
        Ok(self)
    }

    /// Starts building a transaction invocation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownTx`] if `name` is not a transaction declared
    /// by the protocol.
    pub fn tx(&self, name: impl Into<String>) -> Result<TxBuilder, Error> {
        let name = name.into();
        let tir = self
            .transactions
            .get(&name)
            .cloned()
            .ok_or(Error::UnknownTx(name))?;

        Ok(TxBuilder::new(tir, self.trp.clone())
            .env(self.env())
            .parties(self.merged_parties()))
    }

    fn env(&self) -> EnvMap {
        let mut env = self
            .selected_profile
            .as_ref()
            .map(|profile| profile.environment.clone())
            .unwrap_or_default();
        for (key, value) in &self.env_overrides {
            env.insert(key.clone(), value.clone());
        }
        env
    }

    fn merged_parties(&self) -> HashMap<String, Party> {
        let mut merged = HashMap::new();
        if let Some(profile) = &self.selected_profile {
            for (name, address) in &profile.parties {
                merged.insert(name.to_lowercase(), Party::address(address.clone()));
            }
        }
        for (name, party) in &self.bound_parties {
            merged.insert(name.clone(), party.clone());
        }
        merged
    }
}

/// Builder for [`Tx3Client`].
///
/// Obtained via [`Protocol::client`]. All fallible validation — verifying
/// that the selected profile exists, that every bound party is declared by
/// the protocol — happens in [`Tx3ClientBuilder::build`]. Setters never
/// return `Result`, so chains stay fluent.
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::tii::Protocol;
/// use tx3_sdk::{Party};
///
/// let client = Protocol::from_file("protocol.tii")?
///     .client()
///     .trp_endpoint("https://trp.example")
///     .with_profile("preprod")
///     .with_party("sender", Party::address("addr_test1..."))
///     .build()?;
/// ```
pub struct Tx3ClientBuilder {
    transactions: HashMap<String, TirEnvelope>,
    profiles: HashMap<String, Profile>,
    known_parties: HashSet<String>,
    trp_options: Option<trp::ClientOptions>,
    profile: Option<String>,
    parties: HashMap<String, Party>,
    unchecked_parties: HashMap<String, Party>,
    env_overrides: EnvMap,
}

impl Tx3ClientBuilder {
    /// Seeds a builder with already-deconstructed protocol fragments. This is
    /// the entry point used by codegen-generated bindings, which embed only
    /// the runtime essentials at codegen time (per-tx TIR envelopes,
    /// per-profile environment + party-address maps, declared party names)
    /// and avoid carrying the rest of the TII document into the generated
    /// crate.
    pub fn from_parts(
        transactions: HashMap<String, TirEnvelope>,
        profiles: HashMap<String, Profile>,
        known_parties: HashSet<String>,
    ) -> Self {
        let known_parties = known_parties
            .into_iter()
            .map(|name| name.to_lowercase())
            .collect();
        Self {
            transactions,
            profiles,
            known_parties,
            trp_options: None,
            profile: None,
            parties: HashMap::new(),
            unchecked_parties: HashMap::new(),
            env_overrides: EnvMap::new(),
        }
    }

    pub(crate) fn from_protocol(protocol: Protocol) -> Self {
        let transactions = protocol
            .txs()
            .iter()
            .map(|(name, tx)| (name.clone(), tx.tir.clone()))
            .collect();

        let profiles = protocol
            .profiles()
            .iter()
            .map(|(name, profile)| {
                let environment =
                    profile.environment.as_object().cloned().unwrap_or_default();
                (
                    name.clone(),
                    Profile {
                        environment,
                        parties: profile.parties.clone(),
                    },
                )
            })
            .collect();

        let known_parties = protocol.parties().keys().cloned().collect();

        Self::from_parts(transactions, profiles, known_parties)
    }

    /// Sets the full TRP client options.
    pub fn trp(mut self, opts: trp::ClientOptions) -> Self {
        self.trp_options = Some(opts);
        self
    }

    /// Sets the TRP endpoint URL (no headers). Overwrites any previously
    /// supplied options.
    pub fn trp_endpoint(mut self, url: impl Into<String>) -> Self {
        self.trp_options = Some(trp::ClientOptions {
            endpoint: url.into(),
            headers: None,
        });
        self
    }

    /// Adds a header to the TRP client. Initializes the TRP options to an
    /// empty endpoint if not yet set — callers must still supply an endpoint
    /// via [`Tx3ClientBuilder::trp`] or [`Tx3ClientBuilder::trp_endpoint`].
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let opts = self.trp_options.get_or_insert_with(|| trp::ClientOptions {
            endpoint: String::new(),
            headers: None,
        });
        opts.headers
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Selects a profile by name. Validated in `build()`.
    pub fn with_profile(mut self, name: impl Into<String>) -> Self {
        self.profile = Some(name.into());
        self
    }

    /// Binds a party (signer or read-only address) by name. Validated in
    /// `build()` against the protocol's declared parties.
    pub fn with_party(mut self, name: impl Into<String>, party: Party) -> Self {
        self.parties.insert(name.into().to_lowercase(), party);
        self
    }

    /// Binds a party without validating the name against the protocol's
    /// declared parties. The entry is carried straight through to the built
    /// client.
    ///
    /// Intended for codegen-generated wrappers, which materialize one typed
    /// setter per declared party — the name is baked in at codegen time, so
    /// runtime validation would always pass and the embedded party-name set
    /// can be omitted. Hand-written code SHOULD use [`Tx3ClientBuilder::with_party`].
    pub fn with_party_unchecked(mut self, name: impl Into<String>, party: Party) -> Self {
        self.unchecked_parties
            .insert(name.into().to_lowercase(), party);
        self
    }

    /// Binds multiple parties at once.
    pub fn with_parties<I, K>(mut self, parties: I) -> Self
    where
        I: IntoIterator<Item = (K, Party)>,
        K: Into<String>,
    {
        for (name, party) in parties {
            self = self.with_party(name, party);
        }
        self
    }

    /// Sets a single environment value. Merged on top of the selected
    /// profile's environment at resolve time (override wins).
    pub fn with_env_value(
        mut self,
        key: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        self.env_overrides.insert(key.into(), value.into());
        self
    }

    /// Validates the builder state and materializes the [`Tx3Client`].
    ///
    /// # Errors
    ///
    /// - [`Error::Trp`] if no TRP endpoint was supplied.
    /// - [`Error::UnknownProfile`] if the selected profile is not declared
    ///   by the protocol.
    /// - [`Error::UnknownParty`] if any bound party is not declared by the
    ///   protocol.
    pub fn build(self) -> Result<Tx3Client, Error> {
        let trp_options = self.trp_options.ok_or(Error::MissingTrpEndpoint)?;
        if trp_options.endpoint.is_empty() {
            return Err(Error::MissingTrpEndpoint);
        }

        let selected_profile = match self.profile {
            Some(name) => Some(
                self.profiles
                    .get(&name)
                    .cloned()
                    .ok_or(Error::UnknownProfile(name))?,
            ),
            None => None,
        };

        for name in self.parties.keys() {
            if !self.known_parties.contains(name) {
                return Err(Error::UnknownParty(name.clone()));
            }
        }

        let trp = trp::Client::new(trp_options);

        let mut bound_parties = self.parties;
        bound_parties.extend(self.unchecked_parties);

        Ok(Tx3Client::from_parts(
            self.transactions,
            self.known_parties,
            trp,
            bound_parties,
            selected_profile,
            self.env_overrides,
        ))
    }
}

/// Assembles the TRP resolve request shared by every [`TxBuilder`].
///
/// `env` (profile values, with any profile-declared party addresses already
/// folded in), bound party addresses, and caller-supplied `args` are merged
/// into a single argument map, in increasing order of precedence. The request
/// `env` is left unset — TRP receives one argument map.
fn build_resolve_params(
    tir: TirEnvelope,
    env: EnvMap,
    parties: &HashMap<String, Party>,
    args: ArgMap,
) -> ResolveParams {
    let mut merged = ArgMap::new();
    merged.extend(env);
    for (name, party) in parties {
        merged.insert(
            name.clone(),
            Value::String(party.address_value().to_string()),
        );
    }
    merged.extend(args);

    ResolveParams {
        tir,
        args: merged,
        env: None,
    }
}

/// Builder for transaction invocation.
///
/// A builder is a TIR envelope plus the environment, arguments, and party
/// bindings needed to resolve it. Generated codegen clients construct one via
/// [`TxBuilder::new`]; the dynamic [`Tx3Client`] constructs one by adapting a
/// loaded [`Protocol`]. Both drive an identical resolve path.
pub struct TxBuilder {
    tir: TirEnvelope,
    env: EnvMap,
    trp: trp::Client,
    args: ArgMap,
    parties: HashMap<String, Party>,
}

impl TxBuilder {
    /// Creates a builder from a TIR envelope.
    ///
    /// This is the entry point used by generated codegen clients: they bake the
    /// per-transaction TIR and profile data into the generated source at
    /// codegen time and drive the full `resolve → sign → submit → wait`
    /// lifecycle without loading a `.tii` file. Supply environment values with
    /// [`TxBuilder::env`] and signer/address bindings with [`TxBuilder::parties`].
    pub fn new(tir: TirEnvelope, trp: trp::Client) -> Self {
        TxBuilder {
            tir,
            env: EnvMap::new(),
            trp,
            args: ArgMap::new(),
            parties: HashMap::new(),
        }
    }

    /// Sets the environment values applied to this transaction.
    pub fn env(mut self, env: EnvMap) -> Self {
        self.env = env;
        self
    }

    /// Attaches party definitions (signers or read-only addresses).
    ///
    /// Names are matched case-insensitively. Later entries override earlier
    /// ones with the same name.
    pub fn parties(mut self, parties: HashMap<String, Party>) -> Self {
        for (name, party) in parties {
            self.parties.insert(name.to_lowercase(), party);
        }
        self
    }

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
        let TxBuilder {
            tir,
            env,
            trp,
            args,
            parties,
        } = self;

        let resolve_params = build_resolve_params(tir, env, &parties, args);

        let envelope = trp.resolve(resolve_params).await?;

        let signers = parties
            .iter()
            .filter_map(|(name, party)| party.signer_party(name))
            .collect();

        Ok(ResolvedTx {
            trp,
            hash: envelope.hash,
            tx_hex: envelope.tx,
            signers,
            manual_witnesses: Vec::new(),
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
    manual_witnesses: Vec<TxWitness>,
}

impl ResolvedTx {
    /// Returns the transaction hash that signers will sign.
    pub fn signing_hash(&self) -> &str {
        &self.hash
    }

    /// Attaches a pre-computed witness produced outside any registered `Signer`.
    ///
    /// This is the canonical entry point for wallet-app integrations: the consumer
    /// hands `txHex` (or `hash`) to an external wallet, gets back a witness, and
    /// attaches it before calling `sign()`. The witness is appended to the TRP
    /// `SubmitParams.witnesses` array after any witnesses produced by registered
    /// signer parties, in attach order. May be called any number of times.
    ///
    /// The SDK does not verify the witness against the tx hash; that binding is
    /// enforced by TRP at submit time.
    pub fn add_witness(mut self, witness: TxWitness) -> Self {
        self.manual_witnesses.push(witness);
        self
    }

    /// Signs the transaction with every signer party.
    ///
    /// Manually attached witnesses (via `add_witness`) are appended after
    /// witnesses produced by registered signer parties, in attach order.
    /// Succeeds with zero registered signers when at least one witness has
    /// been manually attached.
    pub fn sign(self) -> Result<SignedTx, Error> {
        let total = self.signers.len() + self.manual_witnesses.len();
        let mut witnesses = Vec::with_capacity(total);
        let mut witnesses_info = Vec::with_capacity(total);

        let request = SignRequest {
            tx_hash_hex: self.hash.clone(),
            tx_cbor_hex: self.tx_hex.clone(),
        };

        for signer_party in &self.signers {
            let witness = signer_party
                .signer
                .sign(&request)
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

        for witness in self.manual_witnesses {
            witnesses_info.push(WitnessInfo {
                party: "<external>".to_string(),
                address: String::new(),
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
    use super::{SignRequest, Signer};
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
            request: &SignRequest,
        ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>> {
            let hash_bytes = hex::decode(&request.tx_hash_hex).map_err(|err| {
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
            .derive(DerivationScheme::V2, HARDENED)
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
            request: &SignRequest,
        ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>> {
            let hash_bytes = hex::decode(&request.tx_hash_hex).map_err(|err| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trp::{ClientOptions, WitnessType};

    fn stub_trp() -> trp::Client {
        trp::Client::new(ClientOptions {
            endpoint: "http://localhost:0/unused".to_string(),
            headers: None,
        })
    }

    fn fake_witness(key_hex: &str, sig_hex: &str) -> TxWitness {
        TxWitness {
            key: BytesEnvelope {
                content: key_hex.to_string(),
                content_type: "hex".to_string(),
            },
            signature: BytesEnvelope {
                content: sig_hex.to_string(),
                content_type: "hex".to_string(),
            },
            witness_type: WitnessType::VKey,
        }
    }

    fn empty_resolved() -> ResolvedTx {
        ResolvedTx {
            trp: stub_trp(),
            hash: "deadbeef".to_string(),
            tx_hex: "84a40081".to_string(),
            signers: Vec::new(),
            manual_witnesses: Vec::new(),
        }
    }

    struct StubSigner {
        address: String,
        witness: TxWitness,
    }

    impl Signer for StubSigner {
        fn address(&self) -> &str {
            &self.address
        }

        fn sign(
            &self,
            _request: &SignRequest,
        ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self.witness.clone())
        }
    }

    #[test]
    fn add_witness_only_no_signers() {
        let witness = fake_witness("aa", "bb");
        let signed = empty_resolved()
            .add_witness(witness.clone())
            .sign()
            .expect("sign with manual witness only must succeed");

        assert_eq!(signed.submit.witnesses.len(), 1);
        assert_eq!(signed.submit.witnesses[0].key.content, witness.key.content);
        assert_eq!(
            signed.submit.witnesses[0].signature.content,
            witness.signature.content
        );
    }

    #[test]
    fn add_witness_mixed_with_registered_signer() {
        let registered_witness = fake_witness("11", "22");
        let manual_witness = fake_witness("aa", "bb");

        let stub = StubSigner {
            address: "addr_test1...".to_string(),
            witness: registered_witness.clone(),
        };

        let resolved = ResolvedTx {
            trp: stub_trp(),
            hash: "deadbeef".to_string(),
            tx_hex: "84a40081".to_string(),
            signers: vec![SignerParty {
                name: "sender".to_string(),
                address: stub.address.clone(),
                signer: Arc::new(stub),
            }],
            manual_witnesses: Vec::new(),
        };

        let signed = resolved
            .add_witness(manual_witness.clone())
            .sign()
            .expect("sign with mixed witnesses must succeed");

        assert_eq!(signed.submit.witnesses.len(), 2);
        assert_eq!(signed.submit.witnesses[0].key.content, "11");
        assert_eq!(signed.submit.witnesses[1].key.content, "aa");
    }

    #[test]
    fn add_witness_preserves_attach_order() {
        let signed = empty_resolved()
            .add_witness(fake_witness("01", "10"))
            .add_witness(fake_witness("02", "20"))
            .add_witness(fake_witness("03", "30"))
            .sign()
            .expect("sign must succeed");

        let keys: Vec<&str> = signed
            .submit
            .witnesses
            .iter()
            .map(|w| w.key.content.as_str())
            .collect();
        assert_eq!(keys, vec!["01", "02", "03"]);
    }

    fn sample_tir() -> TirEnvelope {
        TirEnvelope {
            content: "abcd".to_string(),
            encoding: crate::core::TirEncoding::Hex,
            version: "v1beta0".to_string(),
        }
    }

    #[test]
    fn resolve_params_merges_env_parties_and_args() {
        let mut env = EnvMap::new();
        env.insert("network".to_string(), serde_json::json!("testnet"));

        let mut parties = HashMap::new();
        parties.insert("receiver".to_string(), Party::address("addr_receiver"));

        let mut args = ArgMap::new();
        args.insert("quantity".to_string(), serde_json::json!(10_000_000));

        let params = build_resolve_params(sample_tir(), env, &parties, args);

        assert_eq!(params.env, None);
        assert_eq!(params.tir.content, "abcd");
        assert_eq!(params.args.get("network").unwrap(), &serde_json::json!("testnet"));
        assert_eq!(
            params.args.get("receiver").unwrap(),
            &serde_json::json!("addr_receiver")
        );
        assert_eq!(
            params.args.get("quantity").unwrap(),
            &serde_json::json!(10_000_000)
        );
    }

    #[test]
    fn resolve_params_args_override_env() {
        let mut env = EnvMap::new();
        env.insert("quantity".to_string(), serde_json::json!(1));

        let mut args = ArgMap::new();
        args.insert("quantity".to_string(), serde_json::json!(999));

        let params =
            build_resolve_params(sample_tir(), env, &HashMap::new(), args);

        assert_eq!(
            params.args.get("quantity").unwrap(),
            &serde_json::json!(999)
        );
    }

    #[test]
    fn resolve_params_uses_signer_party_address() {
        let stub = StubSigner {
            address: "addr_signer".to_string(),
            witness: fake_witness("aa", "bb"),
        };

        let mut parties = HashMap::new();
        parties.insert("sender".to_string(), Party::signer(stub));

        let params = build_resolve_params(
            sample_tir(),
            EnvMap::new(),
            &parties,
            ArgMap::new(),
        );

        assert_eq!(
            params.args.get("sender").unwrap(),
            &serde_json::json!("addr_signer")
        );
    }
}
