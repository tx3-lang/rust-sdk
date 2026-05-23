//! Support types for generated codegen clients.
//!
//! `trix codegen` produces a typed `Client` per protocol that owns the full
//! transaction lifecycle. This module provides the protocol-agnostic state
//! (TRP client, party bindings, selected profile) and a TIR-driven entry
//! point that the generated `Client` wraps; the codegen template only needs
//! to embed the per-transaction TIR, the typed param structs, and the profile
//! JSON.

use std::collections::HashMap;

use serde::Deserialize;

use crate::core::{EnvMap, TirEnvelope};
use crate::facade::{Party, TxBuilder};
use crate::trp::{self, ClientOptions};

/// A named profile baked into a generated client: environment values and
/// party addresses keyed by name.
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
    /// Parses a JSON map of profiles, the shape the codegen template embeds.
    ///
    /// # Panics
    ///
    /// Panics if the JSON does not parse — codegen produces a valid map by
    /// construction, so a panic indicates a bug in the template.
    pub fn load_all(json: &str) -> HashMap<String, Profile> {
        serde_json::from_str(json).expect("codegen: invalid embedded profiles")
    }
}

/// Backing state for a generated codegen client.
///
/// Holds the TRP connection, party bindings, and the selected profile. The
/// generated `Client` exposes typed per-transaction methods that build a
/// [`TxBuilder`] from this state plus an embedded TIR envelope.
pub struct ProtocolClient {
    trp: trp::Client,
    parties: HashMap<String, Party>,
    profile: Option<Profile>,
}

impl ProtocolClient {
    /// Creates a client over the given TRP options.
    pub fn new(options: ClientOptions) -> Self {
        Self {
            trp: trp::Client::new(options),
            parties: HashMap::new(),
            profile: None,
        }
    }

    /// Applies a profile — its environment values and party addresses apply
    /// to every subsequent transaction.
    pub fn with_profile(mut self, profile: Profile) -> Self {
        self.profile = Some(profile);
        self
    }

    /// Binds a party (signer or read-only address) by name, overriding any
    /// address the selected profile declared for the same name.
    pub fn with_party(mut self, name: impl Into<String>, party: Party) -> Self {
        self.parties.insert(name.into().to_lowercase(), party);
        self
    }

    /// Starts a [`TxBuilder`] for the given TIR envelope, with this client's
    /// environment and party bindings already applied. The caller adds typed
    /// args before driving the lifecycle chain.
    pub fn tx(&self, tir: TirEnvelope) -> TxBuilder {
        TxBuilder::new(tir, self.trp.clone())
            .env(self.env())
            .parties(self.merged_parties())
    }

    fn env(&self) -> EnvMap {
        self.profile
            .as_ref()
            .map(|profile| profile.environment.clone())
            .unwrap_or_default()
    }

    fn merged_parties(&self) -> HashMap<String, Party> {
        let mut merged = HashMap::new();
        if let Some(profile) = &self.profile {
            for (name, address) in &profile.parties {
                merged.insert(name.to_lowercase(), Party::address(address.clone()));
            }
        }
        for (name, party) in &self.parties {
            merged.insert(name.clone(), party.clone());
        }
        merged
    }
}
