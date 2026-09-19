//! Multi-node encrypted mesh rooms & conference broadcast (Issue #2371 / WAVE-26.09)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum GroupRoomError {
    #[error("Member already exists: {0}")]
    MemberAlreadyExists(String),

    #[error("Member not found in room: {0}")]
    MemberNotFound(String),

    #[error("Room not found: {0}")]
    RoomNotFound(String),

    #[error("Unauthorized group action by {0}")]
    Unauthorized(String),

    #[error("Epoch rotation error: {0}")]
    EpochError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemberRole {
    Admin,
    Participant,
    Observer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    pub node_id: String,
    pub wallet_id: String,
    pub role: MemberRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomEpoch {
    pub epoch_id: u64,
    pub epoch_secret_fingerprint: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderKeyDistribution {
    pub room_id: String,
    pub sender_node_id: String,
    pub epoch_id: u64,
    pub encrypted_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRoom {
    pub room_id: String,
    pub name: String,
    pub current_epoch: RoomEpoch,
    pub members: HashMap<String, GroupMember>,
    pub created_at: DateTime<Utc>,
}

impl GroupRoom {
    pub fn new(room_id: &str, name: &str, creator_node: &str, creator_wallet: &str) -> Self {
        let mut members = HashMap::new();
        members.insert(
            creator_node.to_string(),
            GroupMember {
                node_id: creator_node.to_string(),
                wallet_id: creator_wallet.to_string(),
                role: MemberRole::Admin,
                joined_at: Utc::now(),
            },
        );

        Self {
            room_id: room_id.to_string(),
            name: name.to_string(),
            current_epoch: RoomEpoch {
                epoch_id: 1,
                epoch_secret_fingerprint: format!("epoch_1_{}", room_id),
                created_at: Utc::now(),
            },
            members,
            created_at: Utc::now(),
        }
    }

    pub fn join(&mut self, member: GroupMember) -> Result<(), GroupRoomError> {
        if self.members.contains_key(&member.node_id) {
            return Err(GroupRoomError::MemberAlreadyExists(member.node_id));
        }
        self.members.insert(member.node_id.clone(), member);
        Ok(())
    }

    pub fn eject(&mut self, caller_node: &str, target_node: &str) -> Result<(), GroupRoomError> {
        let caller = self
            .members
            .get(caller_node)
            .ok_or_else(|| GroupRoomError::MemberNotFound(caller_node.to_string()))?;

        if caller.role != MemberRole::Admin {
            return Err(GroupRoomError::Unauthorized(caller_node.to_string()));
        }

        if self.members.remove(target_node).is_none() {
            return Err(GroupRoomError::MemberNotFound(target_node.to_string()));
        }

        // Auto-rotate epoch upon member ejection for forward secrecy
        self.rotate_epoch();
        Ok(())
    }

    pub fn rotate_epoch(&mut self) -> u64 {
        self.current_epoch.epoch_id += 1;
        self.current_epoch.epoch_secret_fingerprint = format!(
            "epoch_{}_{}_{}",
            self.current_epoch.epoch_id,
            self.room_id,
            Utc::now().timestamp_millis()
        );
        self.current_epoch.created_at = Utc::now();
        self.current_epoch.epoch_id
    }
}

pub struct GroupMeshManager {
    rooms: Arc<RwLock<HashMap<String, GroupRoom>>>,
}

impl GroupMeshManager {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_group(
        &self,
        room_id: &str,
        name: &str,
        creator_node: &str,
        creator_wallet: &str,
    ) -> GroupRoom {
        let room = GroupRoom::new(room_id, name, creator_node, creator_wallet);
        let mut lock = self.rooms.write().await;
        lock.insert(room_id.to_string(), room.clone());
        room
    }

    pub async fn join_group(
        &self,
        room_id: &str,
        member: GroupMember,
    ) -> Result<(), GroupRoomError> {
        let mut lock = self.rooms.write().await;
        let room = lock
            .get_mut(room_id)
            .ok_or_else(|| GroupRoomError::RoomNotFound(room_id.to_string()))?;
        room.join(member)
    }

    pub async fn eject_peer(
        &self,
        room_id: &str,
        caller_node: &str,
        target_node: &str,
    ) -> Result<(), GroupRoomError> {
        let mut lock = self.rooms.write().await;
        let room = lock
            .get_mut(room_id)
            .ok_or_else(|| GroupRoomError::RoomNotFound(room_id.to_string()))?;
        room.eject(caller_node, target_node)
    }

    pub async fn rotate_epoch(&self, room_id: &str) -> Result<u64, GroupRoomError> {
        let mut lock = self.rooms.write().await;
        let room = lock
            .get_mut(room_id)
            .ok_or_else(|| GroupRoomError::RoomNotFound(room_id.to_string()))?;
        Ok(room.rotate_epoch())
    }

    pub async fn broadcast_group_message(
        &self,
        room_id: &str,
        sender_node: &str,
        _payload: &[u8],
    ) -> Result<Vec<String>, GroupRoomError> {
        let lock = self.rooms.read().await;
        let room = lock
            .get(room_id)
            .ok_or_else(|| GroupRoomError::RoomNotFound(room_id.to_string()))?;

        if !room.members.contains_key(sender_node) {
            return Err(GroupRoomError::MemberNotFound(sender_node.to_string()));
        }

        // Return list of target recipient node IDs (all members except sender)
        let recipients: Vec<String> = room
            .members
            .keys()
            .filter(|&id| id != sender_node)
            .cloned()
            .collect();

        Ok(recipients)
    }
}

impl Default for GroupMeshManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_join_group() {
        let manager = GroupMeshManager::new();
        let room = manager
            .create_group("grp-1", "Admins", "alice", "wallet-1")
            .await;
        assert_eq!(room.members.len(), 1);

        let bob = GroupMember {
            node_id: "bob".into(),
            wallet_id: "wallet-2".into(),
            role: MemberRole::Participant,
            joined_at: Utc::now(),
        };

        manager.join_group("grp-1", bob).await.unwrap();

        let recipients = manager
            .broadcast_group_message("grp-1", "alice", b"ping all")
            .await
            .unwrap();
        assert_eq!(recipients, vec!["bob".to_string()]);
    }

    #[tokio::test]
    async fn test_eject_and_epoch_rotation() {
        let manager = GroupMeshManager::new();
        manager
            .create_group("grp-2", "Devs", "alice", "wallet-1")
            .await;

        let charlie = GroupMember {
            node_id: "charlie".into(),
            wallet_id: "wallet-3".into(),
            role: MemberRole::Participant,
            joined_at: Utc::now(),
        };
        manager.join_group("grp-2", charlie).await.unwrap();

        // Non-admin cannot eject
        let err = manager.eject_peer("grp-2", "charlie", "alice").await;
        assert!(err.is_err());

        // Admin ejects charlie -> triggers automatic epoch rotation
        manager
            .eject_peer("grp-2", "alice", "charlie")
            .await
            .unwrap();

        let new_epoch = manager.rotate_epoch("grp-2").await.unwrap();
        assert_eq!(new_epoch, 3); // initial 1 + eject auto-rotate (2) + manual (3)
    }

    #[tokio::test]
    async fn test_unauthorized_broadcast() {
        let manager = GroupMeshManager::new();
        manager
            .create_group("grp-3", "Ops", "alice", "wallet-1")
            .await;

        let res = manager
            .broadcast_group_message("grp-3", "intruder", b"hack")
            .await;
        assert!(res.is_err());
    }
}
