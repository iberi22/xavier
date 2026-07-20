//! Checkpoint Module Tests

#[cfg(test)]
mod checkpoint_tests {
    use xavier::checkpoint::{
        Checkpoint, CheckpointManager, SessionCheckpoint, MAX_SESSION_CHECKPOINT_BYTES,
    };

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = Checkpoint::new(
            "task_123".to_string(),
            "checkpoint_1".to_string(),
            serde_json::json!({"state": "test"}),
        );

        assert_eq!(checkpoint.task_id, "task_123");
        assert_eq!(checkpoint.name, "checkpoint_1");
    }

    #[test]
    fn test_checkpoint_data() {
        let data = serde_json::json!({
            "step": 1,
            "progress": 0.5,
            "data": "test"
        });

        let checkpoint = Checkpoint::new("task".to_string(), "cp".to_string(), data);

        assert_eq!(checkpoint.data["step"], 1);
    }

    #[tokio::test]
    async fn test_checkpoint_manager_save() {
        let manager = CheckpointManager::new();

        let checkpoint = Checkpoint::new(
            "task_1".to_string(),
            "save_test".to_string(),
            serde_json::json!({"value": 42}),
        );

        manager.save(checkpoint).await.expect("test assertion");

        let loaded = manager
            .load("task_1".to_string(), "save_test".to_string())
            .await
            .expect("test assertion");
        assert!(loaded.is_some());
    }

    #[tokio::test]
    async fn test_checkpoint_list() {
        let manager = CheckpointManager::new();

        // Save multiple checkpoints
        for i in 0..5 {
            let cp = Checkpoint::new(
                "task_list".to_string(),
                format!("cp_{}", i),
                serde_json::json!({"index": i}),
            );
            manager.save(cp).await.expect("test assertion");
        }

        let checkpoints = manager
            .list("task_list".to_string())
            .await
            .expect("test assertion");
        assert_eq!(checkpoints.len(), 5);
    }

    #[tokio::test]
    async fn test_checkpoint_delete() {
        let manager = CheckpointManager::new();

        let cp = Checkpoint::new(
            "task_del".to_string(),
            "to_delete".to_string(),
            serde_json::json!({"test": true}),
        );

        manager.save(cp).await.expect("test assertion");
        manager
            .delete("task_del".to_string(), "to_delete".to_string())
            .await
            .expect("test assertion");

        let loaded = manager
            .load("task_del".to_string(), "to_delete".to_string())
            .await
            .expect("test assertion");
        assert!(loaded.is_none());
    }

    #[test]
    fn test_session_checkpoint_round_trip() {
        let checkpoint = SessionCheckpoint::from_session(
            "session_1",
            "Completed checkpoint system work",
            vec!["src/checkpoint/session.rs".to_string()],
            vec!["git commit -m \"feat(checkpoint): add session continuity\"".to_string()],
            vec!["Implement Phase 3".to_string()],
        )
        .expect("test assertion");

        let payload = checkpoint.to_bytes().expect("test assertion");
        let restored = SessionCheckpoint::from_bytes(&payload).expect("test assertion");

        assert_eq!(restored.session_id, "session_1");
        assert_eq!(restored.file_edits, checkpoint.file_edits);
        assert_eq!(restored.git_operations, checkpoint.git_operations);
        assert_eq!(restored.tasks, checkpoint.tasks);
    }

    #[test]
    fn test_session_checkpoint_budget() {
        let large = (0..20)
            .map(|idx| format!("entry-{idx}-{}", "x".repeat(300)))
            .collect::<Vec<_>>();

        let checkpoint = SessionCheckpoint::from_session(
            "session_budget",
            "y".repeat(3_000),
            large.clone(),
            large.clone(),
            large,
        )
        .expect("test assertion");

        assert!(checkpoint.size_bytes().expect("test assertion") <= MAX_SESSION_CHECKPOINT_BYTES);
    }
}

#[cfg(test)]
mod checkpoint_recovery_tests {
    use xavier::checkpoint::{Checkpoint, CheckpointManager};

    struct MockTask {
        id: String,
        current_step: u32,
        data: String,
    }

    impl MockTask {
        fn new(id: String, data: String) -> Self {
            Self {
                id,
                current_step: 0,
                data,
            }
        }

        async fn run_step(&mut self, manager: &CheckpointManager) -> Result<(), anyhow::Error> {
            self.current_step += 1;
            let checkpoint = Checkpoint::new(
                self.id.clone(),
                format!("step_{}", self.current_step),
                serde_json::json!({
                    "step": self.current_step,
                    "data": self.data
                }),
            );
            manager.save(checkpoint).await?;
            Ok(())
        }

        async fn recover(&mut self, manager: &CheckpointManager, step_name: &str) -> Result<(), anyhow::Error> {
            if let Some(checkpoint) = manager.load(self.id.clone(), step_name.to_string()).await? {
                self.current_step = checkpoint.data["step"].as_u64().unwrap_or(0) as u32;
                self.data = checkpoint.data["data"].as_str().unwrap_or("").to_string();
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_full_recovery() {
        let manager = CheckpointManager::new();
        let mut task = MockTask::new("task_recovery_1".to_string(), "step1_data".to_string());

        // Run step 1
        task.run_step(&manager).await.unwrap();
        assert_eq!(task.current_step, 1);

        // Modify task state to simulate a crash/loss of memory state
        task.current_step = 0;
        task.data = "corrupted".to_string();

        // Recover to step_1
        task.recover(&manager, "step_1").await.unwrap();
        assert_eq!(task.current_step, 1);
        assert_eq!(task.data, "step1_data");
    }

    #[tokio::test]
    async fn test_partial_recovery() {
        let manager = CheckpointManager::new();
        let mut task = MockTask::new("task_recovery_2".to_string(), "step2_data".to_string());

        // Save multiple steps
        task.run_step(&manager).await.unwrap(); // step_1
        task.data = "step2_data_new".to_string();
        task.run_step(&manager).await.unwrap(); // step_2

        // Recover specifically to step_1
        let mut restored_task = MockTask::new("task_recovery_2".to_string(), "".to_string());
        restored_task.recover(&manager, "step_1").await.unwrap();
        assert_eq!(restored_task.current_step, 1);
        assert_eq!(restored_task.data, "step2_data");

        // Recover specifically to step_2
        let mut restored_task_2 = MockTask::new("task_recovery_2".to_string(), "".to_string());
        restored_task_2.recover(&manager, "step_2").await.unwrap();
        assert_eq!(restored_task_2.current_step, 2);
        assert_eq!(restored_task_2.data, "step2_data_new");
    }
}
