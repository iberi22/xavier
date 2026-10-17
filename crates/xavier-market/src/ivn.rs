//! MKT-17 IVN state: a validator registry, identity requests and signed votes.
//!
//! A vote is accepted only when it carries an Ed25519 signature, made with the
//! key of a registered validator, over the request id, the validator id and
//! the decision. The signature is checked before the request is looked at, so
//! an unsigned or mis-signed vote never changes state. The store picks each
//! request's validators from the registry, ranked by a hash of the request id,
//! so the applicant cannot choose its own validators.
//!
//! Every write is one SQLite transaction on the same file as the listings, so
//! IVN state survives a restart the same way.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::MarketError;
use crate::store::{non_empty, now_ms, MarketStore};

/// Domain-separation prefix for vote signatures. A signature made for another
/// purpose cannot be replayed as a vote.
pub const VOTE_DOMAIN: &str = "xavier-ivn-vote:v1";

/// A validator's choice on one identity request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteDecision {
    Check,
    Reject,
    Abstain,
}

impl VoteDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Reject => "reject",
            Self::Abstain => "abstain",
        }
    }
}

/// Where an identity request stands. Only `Pending` accepts votes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Pending,
    /// Enough validators checked the applicant to meet the quorum.
    Passed,
    /// The quorum can no longer be met.
    Rejected,
    /// Every assigned validator voted and the quorum was still not met.
    QuorumNotMet,
}

impl RequestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Rejected => "rejected",
            Self::QuorumNotMet => "quorum_not_met",
        }
    }
}

/// A registered validator. Its public key is hex-encoded, 32 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validator {
    pub node_id: String,
    pub public_key: String,
    pub registered_at_ms: i64,
}

/// Input for [`MarketStore::register_validator`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewValidator {
    pub node_id: String,
    pub public_key: String,
}

/// One accepted vote. The signature is kept so any reader can re-verify it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vote {
    pub validator_id: String,
    pub decision: VoteDecision,
    pub signature: String,
    pub cast_at_ms: i64,
}

/// An identity request with its assigned validators and votes so far.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRequest {
    pub id: String,
    pub applicant: String,
    pub status: RequestStatus,
    pub validators: Vec<String>,
    pub votes: Vec<Vote>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Input for [`MarketStore::create_identity_request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewIdentityRequest {
    pub applicant: String,
}

/// A signed vote. `signature` is hex-encoded, 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastVote {
    pub request_id: String,
    pub validator_id: String,
    pub decision: VoteDecision,
    pub signature: String,
}

/// The exact bytes a validator signs for a vote.
pub fn vote_message(request_id: &str, validator_id: &str, decision: VoteDecision) -> Vec<u8> {
    format!(
        "{VOTE_DOMAIN}\n{request_id}\n{validator_id}\n{}",
        decision.as_str()
    )
    .into_bytes()
}

/// Signs a vote with a validator's 32-byte Ed25519 seed and returns the
/// signature as hex. Validators run this on their own node; the store never
/// sees the seed.
pub fn sign_vote(
    seed: &[u8; 32],
    request_id: &str,
    validator_id: &str,
    decision: VoteDecision,
) -> String {
    let key = SigningKey::from_bytes(seed);
    hex::encode(
        key.sign(&vote_message(request_id, validator_id, decision))
            .to_bytes(),
    )
}

/// The hex public key that matches a 32-byte Ed25519 seed.
pub fn public_key_for_seed(seed: &[u8; 32]) -> String {
    hex::encode(SigningKey::from_bytes(seed).verifying_key().to_bytes())
}

/// Decides a request's status from its votes. `validators` is the number
/// assigned and `quorum_permille` the approval share needed to pass.
///
/// Passed once approvals reach the quorum. Rejected once even a unanimous
/// remainder could not reach it. QuorumNotMet once everyone has voted and it
/// still cannot pass. Otherwise Pending.
pub fn evaluate(
    validators: usize,
    decisions: &[VoteDecision],
    quorum_permille: u32,
) -> RequestStatus {
    let assigned = validators as u64;
    let quorum = u64::from(quorum_permille);
    let count = |wanted: VoteDecision| decisions.iter().filter(|d| **d == wanted).count() as u64;
    let approvals = count(VoteDecision::Check);
    let rejects = count(VoteDecision::Reject);

    if approvals * 1000 >= quorum * assigned {
        RequestStatus::Passed
    } else if assigned.saturating_sub(rejects) * 1000 < quorum * assigned {
        RequestStatus::Rejected
    } else if decisions.len() as u64 >= assigned {
        RequestStatus::QuorumNotMet
    } else {
        RequestStatus::Pending
    }
}

impl MarketStore {
    /// Registers a validator with its public key. A node id can be registered
    /// once; re-keying is not supported in v1.
    pub fn register_validator(&self, new: NewValidator) -> Result<Validator, MarketError> {
        let node_id = non_empty("node_id", &new.node_id)?;
        let public_key = normalize_public_key(&new.public_key)?;
        let validator = Validator {
            node_id,
            public_key,
            registered_at_ms: now_ms(),
        };
        let inserted = self.conn().execute(
            "INSERT OR IGNORE INTO ivn_validators (node_id, public_key, registered_at_ms)
             VALUES (?1, ?2, ?3)",
            params![
                validator.node_id,
                validator.public_key,
                validator.registered_at_ms
            ],
        )?;
        if inserted == 0 {
            return Err(MarketError::Conflict(format!(
                "validator {} is already registered",
                validator.node_id
            )));
        }
        Ok(validator)
    }

    /// Every registered validator, oldest registration first.
    pub fn list_validators(&self) -> Result<Vec<Validator>, MarketError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT node_id, public_key, registered_at_ms FROM ivn_validators
             ORDER BY registered_at_ms, rowid",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Validator {
                    node_id: row.get(0)?,
                    public_key: row.get(1)?,
                    registered_at_ms: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Opens an identity request and assigns it `ivn.validators_per_request`
    /// validators from the registry. The choice depends on the server-generated
    /// request id, so the applicant cannot pick or predict its validators.
    pub fn create_identity_request(
        &self,
        new: NewIdentityRequest,
    ) -> Result<IdentityRequest, MarketError> {
        let applicant = non_empty("applicant", &new.applicant)?;
        let needed = self.config().ivn_validators_per_request as usize;
        let id = format!("ivn_req_{}", uuid::Uuid::new_v4());
        let now = now_ms();

        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut ranked: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT node_id FROM ivn_validators")?;
            let node_ids = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            node_ids
                .into_iter()
                .map(|node_id| (rank(&id, &node_id), node_id))
                .collect()
        };
        if ranked.len() < needed {
            return Err(MarketError::Conflict(format!(
                "an identity request needs {needed} registered validators, have {}",
                ranked.len()
            )));
        }
        ranked.sort();
        let mut validators: Vec<String> = ranked
            .into_iter()
            .take(needed)
            .map(|(_, node_id)| node_id)
            .collect();
        // `get_identity_request` returns assignments sorted by id; match it.
        validators.sort();

        tx.execute(
            "INSERT INTO ivn_requests (id, applicant, status, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, applicant, RequestStatus::Pending.as_str(), now],
        )?;
        for validator_id in &validators {
            tx.execute(
                "INSERT INTO ivn_request_validators (request_id, validator_id) VALUES (?1, ?2)",
                params![id, validator_id],
            )?;
        }
        tx.commit()?;

        Ok(IdentityRequest {
            id,
            applicant,
            status: RequestStatus::Pending,
            validators,
            votes: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
        })
    }

    pub fn get_identity_request(&self, id: &str) -> Result<Option<IdentityRequest>, MarketError> {
        let conn = self.conn();
        load_request(&conn, id)
    }

    /// Records one validator's vote, then recomputes the request status.
    ///
    /// Checks run in this order and each failure leaves the store unchanged:
    /// the validator is registered and the signature verifies (401), the
    /// request exists (404), the validator is assigned to it (403), the
    /// request is still pending (409) and this validator has not voted (409).
    pub fn cast_vote(&self, cast: CastVote) -> Result<IdentityRequest, MarketError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        let key_hex: Option<String> = tx
            .query_row(
                "SELECT public_key FROM ivn_validators WHERE node_id = ?1",
                params![cast.validator_id],
                |row| row.get(0),
            )
            .optional()?;
        let key_hex = key_hex.ok_or_else(|| {
            MarketError::Unauthenticated("vote is not signed by a registered validator".into())
        })?;
        let key = decode_verifying_key(&key_hex)?;
        let signature = decode_signature(&cast.signature)?;
        key.verify_strict(
            &vote_message(&cast.request_id, &cast.validator_id, cast.decision),
            &signature,
        )
        .map_err(|_| MarketError::Unauthenticated("vote signature does not verify".into()))?;

        let request = load_request(&tx, &cast.request_id)?
            .ok_or_else(|| MarketError::IdentityRequestNotFound(cast.request_id.clone()))?;
        if !request.validators.contains(&cast.validator_id) {
            return Err(MarketError::Forbidden(format!(
                "validator {} is not assigned to request {}",
                cast.validator_id, cast.request_id
            )));
        }
        if request.status != RequestStatus::Pending {
            return Err(MarketError::Conflict(format!(
                "request {} is already {}",
                cast.request_id,
                request.status.as_str()
            )));
        }
        if request
            .votes
            .iter()
            .any(|vote| vote.validator_id == cast.validator_id)
        {
            return Err(MarketError::Conflict(format!(
                "validator {} has already voted on request {}",
                cast.validator_id, cast.request_id
            )));
        }

        let now = now_ms();
        tx.execute(
            "INSERT INTO ivn_votes (request_id, validator_id, decision, signature, cast_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                cast.request_id,
                cast.validator_id,
                cast.decision.as_str(),
                hex::encode(signature.to_bytes()),
                now,
            ],
        )?;

        let mut decisions: Vec<VoteDecision> =
            request.votes.iter().map(|vote| vote.decision).collect();
        decisions.push(cast.decision);
        let status = evaluate(
            request.validators.len(),
            &decisions,
            self.config().ivn_quorum_permille,
        );
        tx.execute(
            "UPDATE ivn_requests SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![status.as_str(), now, cast.request_id],
        )?;

        let updated = load_request(&tx, &cast.request_id)?.ok_or_else(|| {
            MarketError::Corrupt(format!("request {} vanished mid-vote", cast.request_id))
        })?;
        tx.commit()?;
        Ok(updated)
    }
}

/// Ranks a validator for a request. Hashing the request id with the node id
/// gives each request its own order over the registry.
fn rank(request_id: &str, node_id: &str) -> String {
    hex::encode(Sha256::digest(format!("{request_id}\n{node_id}")))
}

fn load_request(conn: &Connection, id: &str) -> Result<Option<IdentityRequest>, MarketError> {
    let header: Option<(String, String, String, i64, i64)> = conn
        .query_row(
            "SELECT applicant, status, id, created_at_ms, updated_at_ms
             FROM ivn_requests WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((applicant, status, id, created_at_ms, updated_at_ms)) = header else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT validator_id FROM ivn_request_validators WHERE request_id = ?1 ORDER BY validator_id",
    )?;
    let validators = stmt
        .query_map(params![id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT validator_id, decision, signature, cast_at_ms FROM ivn_votes
         WHERE request_id = ?1 ORDER BY cast_at_ms, validator_id",
    )?;
    let votes = stmt
        .query_map(params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .map(|row| {
            let (validator_id, decision, signature, cast_at_ms) = row?;
            Ok(Vote {
                validator_id,
                decision: parse_decision(&decision)?,
                signature,
                cast_at_ms,
            })
        })
        .collect::<Result<Vec<_>, MarketError>>()?;

    Ok(Some(IdentityRequest {
        status: parse_status(&status)?,
        id,
        applicant,
        validators,
        votes,
        created_at_ms,
        updated_at_ms,
    }))
}

fn parse_decision(value: &str) -> Result<VoteDecision, MarketError> {
    match value {
        "check" => Ok(VoteDecision::Check),
        "reject" => Ok(VoteDecision::Reject),
        "abstain" => Ok(VoteDecision::Abstain),
        other => Err(MarketError::Corrupt(format!(
            "unknown vote decision {other:?}"
        ))),
    }
}

fn parse_status(value: &str) -> Result<RequestStatus, MarketError> {
    match value {
        "pending" => Ok(RequestStatus::Pending),
        "passed" => Ok(RequestStatus::Passed),
        "rejected" => Ok(RequestStatus::Rejected),
        "quorum_not_met" => Ok(RequestStatus::QuorumNotMet),
        other => Err(MarketError::Corrupt(format!(
            "unknown request status {other:?}"
        ))),
    }
}

/// Parses and re-encodes a public key so only one spelling is stored.
fn normalize_public_key(value: &str) -> Result<String, MarketError> {
    let bytes = hex::decode(value.trim())
        .map_err(|_| MarketError::InvalidInput("public_key must be hex".into()))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| MarketError::InvalidInput("public_key must be 32 bytes".into()))?;
    VerifyingKey::from_bytes(&array)
        .map_err(|_| MarketError::InvalidInput("public_key is not a valid Ed25519 key".into()))?;
    Ok(hex::encode(array))
}

/// A key read back from the store that fails to parse means the row is corrupt.
fn decode_verifying_key(value: &str) -> Result<VerifyingKey, MarketError> {
    let bytes = hex::decode(value)
        .map_err(|_| MarketError::Corrupt("stored validator key is not hex".into()))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| MarketError::Corrupt("stored validator key is not 32 bytes".into()))?;
    VerifyingKey::from_bytes(&array)
        .map_err(|_| MarketError::Corrupt("stored validator key is not a valid Ed25519 key".into()))
}

/// A malformed signature is an authentication failure, not a bad request, so
/// callers cannot tell a bad signature from a bad encoding.
fn decode_signature(value: &str) -> Result<Signature, MarketError> {
    let bytes = hex::decode(value.trim())
        .map_err(|_| MarketError::Unauthenticated("vote signature is not valid".into()))?;
    let array: [u8; 64] = bytes
        .try_into()
        .map_err(|_| MarketError::Unauthenticated("vote signature is not valid".into()))?;
    Ok(Signature::from_bytes(&array))
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::config::MarketConfig;

    /// Validators are `validator-0`, `validator-1`, ...; each has a seed that
    /// is its index plus one, repeated.
    fn node(index: u8) -> String {
        format!("validator-{index}")
    }

    fn seed(index: u8) -> [u8; 32] {
        [index + 1; 32]
    }

    /// Recovers the seed of a test validator from its node id.
    fn seed_of(node_id: &str) -> [u8; 32] {
        seed(node_id.trim_start_matches("validator-").parse().unwrap())
    }

    fn register(store: &MarketStore, count: u8) {
        register_range(store, 0, count);
    }

    fn register_range(store: &MarketStore, start: u8, end: u8) {
        for index in start..end {
            store
                .register_validator(NewValidator {
                    node_id: node(index),
                    public_key: public_key_for_seed(&seed(index)),
                })
                .unwrap();
        }
    }

    fn open_request(store: &MarketStore) -> IdentityRequest {
        store
            .create_identity_request(NewIdentityRequest {
                applicant: "applicant-1".to_string(),
            })
            .unwrap()
    }

    /// A vote for `validator_id` on `request_id`, signed with its own seed.
    fn vote_for(request_id: &str, validator_id: &str, decision: VoteDecision) -> CastVote {
        CastVote {
            request_id: request_id.to_string(),
            validator_id: validator_id.to_string(),
            decision,
            signature: sign_vote(&seed_of(validator_id), request_id, validator_id, decision),
        }
    }

    fn file_store(dir: &tempfile::TempDir) -> (std::path::PathBuf, MarketStore) {
        let path = dir.path().join("market.sqlite3");
        let store = MarketStore::open(&path, MarketConfig::default()).unwrap();
        (path, store)
    }

    #[test]
    fn evaluate_follows_the_quorum_rule() {
        use VoteDecision::{Abstain, Check, Reject};
        // Five validators at 80%: four approvals pass.
        assert_eq!(evaluate(5, &[Check; 3], 800), RequestStatus::Pending);
        assert_eq!(evaluate(5, &[Check; 4], 800), RequestStatus::Passed);
        // Two rejections leave at most three approvals, one short.
        assert_eq!(evaluate(5, &[Reject], 800), RequestStatus::Pending);
        assert_eq!(evaluate(5, &[Reject, Reject], 800), RequestStatus::Rejected);
        // Everyone voted, nobody rejected, quorum still missed.
        assert_eq!(
            evaluate(5, &[Check, Check, Check, Abstain, Abstain], 800),
            RequestStatus::QuorumNotMet
        );
        assert_eq!(evaluate(5, &[Abstain; 5], 800), RequestStatus::QuorumNotMet);
        // A two-validator request at 50% passes on one approval.
        assert_eq!(evaluate(2, &[Check], 500), RequestStatus::Passed);
        assert_eq!(
            evaluate(3, &[Check, Reject, Reject], 800),
            RequestStatus::Rejected
        );
    }

    #[test]
    fn state_survives_a_restart_and_voting_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let (path, store) = file_store(&dir);
        register(&store, 6);
        let request = open_request(&store);
        let assigned = request.validators.clone();
        assert_eq!(assigned.len(), 5);
        store
            .cast_vote(vote_for(&request.id, &assigned[0], VoteDecision::Check))
            .unwrap();
        drop(store);

        let reopened = MarketStore::open(&path, MarketConfig::default()).unwrap();
        let after_restart = reopened.get_identity_request(&request.id).unwrap().unwrap();
        assert_eq!(after_restart.status, RequestStatus::Pending);
        assert_eq!(after_restart.validators, assigned);
        assert_eq!(after_restart.votes.len(), 1);
        assert_eq!(after_restart.votes[0].validator_id, assigned[0]);

        for validator in &assigned[1..4] {
            reopened
                .cast_vote(vote_for(&request.id, validator, VoteDecision::Check))
                .unwrap();
        }
        let passed = reopened.get_identity_request(&request.id).unwrap().unwrap();
        assert_eq!(passed.status, RequestStatus::Passed);
        assert_eq!(passed.votes.len(), 4);
    }

    #[test]
    fn unsigned_and_forged_votes_are_rejected_and_change_nothing() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 6);
        let request = open_request(&store);
        let voter = request.validators[0].clone();
        let other_request = open_request(&store);
        let intruder = "intruder".to_string();

        let unsigned = CastVote {
            signature: String::new(),
            ..vote_for(&request.id, &voter, VoteDecision::Check)
        };
        let wrong_key = CastVote {
            signature: sign_vote(
                &seed_of(&request.validators[1]),
                &request.id,
                &voter,
                VoteDecision::Check,
            ),
            ..vote_for(&request.id, &voter, VoteDecision::Check)
        };
        let tampered = CastVote {
            decision: VoteDecision::Reject,
            ..vote_for(&request.id, &voter, VoteDecision::Check)
        };
        let replayed = CastVote {
            request_id: request.id.clone(),
            ..vote_for(&other_request.id, &voter, VoteDecision::Check)
        };
        let unregistered = CastVote {
            validator_id: intruder.clone(),
            ..vote_for(&request.id, &voter, VoteDecision::Check)
        };
        for (label, cast) in [
            ("unsigned", unsigned),
            ("wrong key", wrong_key),
            ("tampered decision", tampered),
            ("replayed across requests", replayed),
            ("unregistered validator", unregistered),
        ] {
            let err = store.cast_vote(cast).unwrap_err();
            assert!(
                matches!(err, MarketError::Unauthenticated(_)),
                "{label} vote gave {err:?}"
            );
        }

        let unchanged = store.get_identity_request(&request.id).unwrap().unwrap();
        assert!(unchanged.votes.is_empty());
        assert_eq!(unchanged.status, RequestStatus::Pending);

        // Positive control: the same vote, correctly signed, is accepted.
        let accepted = store
            .cast_vote(vote_for(&request.id, &voter, VoteDecision::Check))
            .unwrap();
        assert_eq!(accepted.votes.len(), 1);
    }

    #[test]
    fn registered_but_unassigned_validator_is_forbidden() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 6);
        let request = open_request(&store);
        let outsider = (0..6)
            .map(node)
            .find(|id| !request.validators.contains(id))
            .unwrap();
        let err = store
            .cast_vote(vote_for(&request.id, &outsider, VoteDecision::Check))
            .unwrap_err();
        assert!(matches!(err, MarketError::Forbidden(_)), "got {err:?}");
    }

    #[test]
    fn a_validator_votes_once_and_a_decided_request_takes_no_more_votes() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 5);
        let request = open_request(&store);
        let first = request.validators[0].clone();
        store
            .cast_vote(vote_for(&request.id, &first, VoteDecision::Abstain))
            .unwrap();
        let err = store
            .cast_vote(vote_for(&request.id, &first, VoteDecision::Check))
            .unwrap_err();
        assert!(matches!(err, MarketError::Conflict(_)), "got {err:?}");

        // Two rejections make approval impossible at 80% of five.
        for validator in &request.validators[1..3] {
            store
                .cast_vote(vote_for(&request.id, validator, VoteDecision::Reject))
                .unwrap();
        }
        let decided = store.get_identity_request(&request.id).unwrap().unwrap();
        assert_eq!(decided.status, RequestStatus::Rejected);

        let late = request.validators[3].clone();
        let err = store
            .cast_vote(vote_for(&request.id, &late, VoteDecision::Check))
            .unwrap_err();
        assert!(matches!(err, MarketError::Conflict(_)), "got {err:?}");
    }

    #[test]
    fn unknown_request_is_not_found_after_a_valid_signature() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 1);
        let err = store
            .cast_vote(vote_for("ivn_req_missing", &node(0), VoteDecision::Check))
            .unwrap_err();
        assert!(
            matches!(err, MarketError::IdentityRequestNotFound(ref id) if id == "ivn_req_missing"),
            "got {err:?}"
        );
    }

    #[test]
    fn stored_votes_cannot_be_edited_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (path, store) = file_store(&dir);
        register(&store, 5);
        let request = open_request(&store);
        store
            .cast_vote(vote_for(
                &request.id,
                &request.validators[0],
                VoteDecision::Check,
            ))
            .unwrap();
        drop(store);

        let raw = Connection::open(&path).unwrap();
        let update = raw
            .execute("UPDATE ivn_votes SET decision = 'reject'", [])
            .unwrap_err();
        assert!(update.to_string().contains("append-only"), "{update}");
        let delete = raw.execute("DELETE FROM ivn_votes", []).unwrap_err();
        assert!(delete.to_string().contains("append-only"), "{delete}");
    }

    #[test]
    fn a_request_needs_enough_registered_validators_and_gets_distinct_ones() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 4);
        let err = store
            .create_identity_request(NewIdentityRequest {
                applicant: "applicant-1".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, MarketError::Conflict(_)), "got {err:?}");

        register_range(&store, 4, 8);
        let first = open_request(&store);
        let second = open_request(&store);
        for request in [&first, &second] {
            let mut unique = request.validators.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), 5);
        }
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn validator_keys_are_checked_and_registration_is_once() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let key = public_key_for_seed(&seed(0));
        let not_hex = store.register_validator(NewValidator {
            node_id: node(0),
            public_key: "zz".to_string(),
        });
        assert!(matches!(not_hex, Err(MarketError::InvalidInput(_))));
        let short = store.register_validator(NewValidator {
            node_id: node(0),
            public_key: key[..62].to_string(),
        });
        assert!(matches!(short, Err(MarketError::InvalidInput(_))));

        let stored = store
            .register_validator(NewValidator {
                node_id: node(0),
                public_key: key.to_uppercase(),
            })
            .unwrap();
        assert_eq!(
            stored.public_key, key,
            "stored key is normalised to lowercase"
        );
        let again = store.register_validator(NewValidator {
            node_id: node(0),
            public_key: key.clone(),
        });
        assert!(matches!(again, Err(MarketError::Conflict(_))));
        assert_eq!(store.list_validators().unwrap(), vec![stored]);
    }

    #[test]
    fn empty_applicant_is_refused() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 5);
        let err = store
            .create_identity_request(NewIdentityRequest {
                applicant: "   ".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, MarketError::InvalidInput(_)), "got {err:?}");
    }

    #[test]
    fn vote_signature_is_bound_to_the_vote_domain() {
        // The same key signs the bare fields without the domain prefix. That
        // signature is valid Ed25519 but must not count as a vote.
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        register(&store, 5);
        let request = open_request(&store);
        let bare = format!("{}\n{}\ncheck", request.id, node(0));
        let key = SigningKey::from_bytes(&seed(0));
        let cast = CastVote {
            request_id: request.id.clone(),
            validator_id: node(0),
            decision: VoteDecision::Check,
            signature: hex::encode(key.sign(bare.as_bytes()).to_bytes()),
        };
        assert!(matches!(
            store.cast_vote(cast),
            Err(MarketError::Unauthenticated(_))
        ));
    }
}
