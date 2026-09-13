//! IntrospectionEngine — LLM-guided human deep thinking sessions.
//!
//! The LLM facilitates, the human produces the insight.
//! 6 techniques: Socratic, 5 Whys, Pre-Mortem, Steel Manning, First Principles, Pattern Recognition.
//!
//! Sessions are stored locally (Privacy P4 — never leaves the node) and
//! high-quality insights become training data when human marks them eligible.

use chrono::Utc;
use tracing::info;

use crate::humanchallenge::{
    store::HumanChallengeStore,
    types::{
        ChallengeType, IntrospectionSession, IntrospectionStatus, IntrospectionTechnique,
        IntrospectionTurn, TurnRole,
    },
};

/// Engine that manages introspection sessions.
pub struct IntrospectionEngine {
    store: std::sync::Arc<HumanChallengeStore>,
}

impl IntrospectionEngine {
    pub fn new(store: std::sync::Arc<HumanChallengeStore>) -> Self {
        Self { store }
    }

    /// Start a new introspection session for a challenge.
    /// Auto-selects technique if not specified.
    pub fn start_session(
        &self,
        challenge_id: &str,
        challenge_type: ChallengeType,
        challenge_description: &str,
        technique: Option<IntrospectionTechnique>,
    ) -> Result<IntrospectionSession, String> {
        let technique =
            technique.unwrap_or_else(|| IntrospectionTechnique::recommend_for(challenge_type));
        let mut session = IntrospectionSession::new(challenge_id, technique);

        // Add the opening LLM turn (the guide sets the frame)
        let opening = self.opening_prompt(&technique, challenge_description);
        session.turns.push(IntrospectionTurn {
            role: TurnRole::LlmGuide,
            content: opening,
            timestamp: Utc::now(),
        });

        self.store
            .save_introspection_session(&session)
            .map_err(|e| e.to_string())?;

        info!(
            "Introspection session {} started for challenge {} using {:?}",
            session.id, challenge_id, technique
        );

        Ok(session)
    }

    /// Process a human turn and generate the next LLM guide prompt.
    /// Returns the updated session with the new LLM response appended.
    pub fn process_turn(
        &self,
        session_id: &str,
        human_input: &str,
        challenge_description: &str,
    ) -> Result<IntrospectionSession, String> {
        let mut session = self
            .store
            .get_introspection_session(session_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        if session.status != IntrospectionStatus::Active {
            return Err(format!("Session {} is not active", session_id));
        }

        // Record the human turn
        session.turns.push(IntrospectionTurn {
            role: TurnRole::Human,
            content: human_input.to_string(),
            timestamp: Utc::now(),
        });

        // Count human turns to determine depth
        let human_turns = session
            .turns
            .iter()
            .filter(|t| t.role == TurnRole::Human)
            .count();

        let llm_response = self.guide_response(
            &session.technique,
            challenge_description,
            human_input,
            human_turns,
        );

        session.turns.push(IntrospectionTurn {
            role: TurnRole::LlmGuide,
            content: llm_response,
            timestamp: Utc::now(),
        });

        // Update depth score
        session.depth_score = session.compute_depth_score();

        // Auto-complete at depth 5 for 5 Whys
        if session.technique == IntrospectionTechnique::FiveWhys && human_turns >= 5 {
            session.status = IntrospectionStatus::Completed;
            session.completed_at = Some(Utc::now());
            session.insights = self.extract_insights(&session);
        }

        self.store
            .save_introspection_session(&session)
            .map_err(|e| e.to_string())?;

        Ok(session)
    }

    /// Complete a session manually and extract insights.
    pub fn complete_session(&self, session_id: &str) -> Result<IntrospectionSession, String> {
        let mut session = self
            .store
            .get_introspection_session(session_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        session.status = IntrospectionStatus::Completed;
        session.completed_at = Some(Utc::now());
        session.depth_score = session.compute_depth_score();
        session.insights = self.extract_insights(&session);

        self.store
            .save_introspection_session(&session)
            .map_err(|e| e.to_string())?;

        info!(
            "Introspection session {} completed with depth_score={:.2} and {} insights",
            session_id,
            session.depth_score,
            session.insights.len()
        );

        Ok(session)
    }

    // -----------------------------------------------------------------------
    // Private: guide response generation
    // -----------------------------------------------------------------------

    fn opening_prompt(&self, technique: &IntrospectionTechnique, description: &str) -> String {
        match technique {
            IntrospectionTechnique::SocraticQuestioning => format!(
                "Vamos a explorar esto con preguntas. Sobre '{}': ¿Qué es lo que más te llama la atención de esta situación?",
                description
            ),
            IntrospectionTechnique::FiveWhys => format!(
                "Vamos a usar los 5 Porqués para llegar a la raíz. Sobre '{}': ¿Por qué ocurrió esto?",
                description
            ),
            IntrospectionTechnique::PreMortem => format!(
                "Hagamos un Pre-Mortem. Imagina que estamos 6 meses en el futuro y '{}' falló completamente. ¿Qué fue lo que salió mal?",
                description
            ),
            IntrospectionTechnique::SteelManning => format!(
                "Construyamos el mejor argumento posible para la posición opuesta a '{}'. ¿Cuál sería el caso más sólido?",
                description
            ),
            IntrospectionTechnique::FirstPrinciples => format!(
                "Descompongamos '{}' hasta los axiomas fundamentales. ¿Cuáles son los supuestos que estás dando por sentados?",
                description
            ),
            IntrospectionTechnique::PatternRecognition => format!(
                "Sobre '{}': ¿Has visto esta situación o algo similar antes? ¿Cuándo fue y qué pasó?",
                description
            ),
        }
    }

    fn guide_response(
        &self,
        technique: &IntrospectionTechnique,
        _description: &str,
        human_input: &str,
        turn_number: usize,
    ) -> String {
        let truncated = if human_input.len() > 80 {
            format!("{}...", &human_input[..80])
        } else {
            human_input.to_string()
        };

        match technique {
            IntrospectionTechnique::FiveWhys => {
                if turn_number >= 5 {
                    format!(
                        "Has llegado al nivel 5. La causa raíz que emergió es: '{}'. ¿Cómo cambiaría esto tu enfoque?",
                        truncated
                    )
                } else {
                    format!(
                        "Interesante — '{}'. Ahora, ¿y por qué es así? (nivel {}/5)",
                        truncated,
                        turn_number + 1
                    )
                }
            }
            IntrospectionTechnique::SocraticQuestioning => {
                format!(
                    "Mencionas '{}'. ¿Qué implicaría si eso fuera cierto en todos los casos? ¿O solo en este?",
                    truncated
                )
            }
            IntrospectionTechnique::PreMortem => {
                format!(
                    "'{}' es un riesgo real. ¿Cuál sería la señal de alarma temprana de que esto está ocurriendo? ¿Qué acción preventiva eliminaría este riesgo?",
                    truncated
                )
            }
            IntrospectionTechnique::SteelManning => {
                format!(
                    "Buen punto: '{}'. Para hacerlo más sólido aún — ¿qué evidencia concreta apoyaría esa posición?",
                    truncated
                )
            }
            IntrospectionTechnique::FirstPrinciples => {
                format!(
                    "Identificas '{}' como supuesto. ¿Es este realmente un axioma o también puede descomponerse más?",
                    truncated
                )
            }
            IntrospectionTechnique::PatternRecognition => {
                format!(
                    "'{}' — ¿cuál fue el resultado de esa vez? ¿Qué fue distinto en el contexto comparado con ahora?",
                    truncated
                )
            }
        }
    }

    fn extract_insights(&self, session: &IntrospectionSession) -> Vec<String> {
        session
            .turns
            .iter()
            .filter(|t| t.role == TurnRole::Human && t.content.len() > 50)
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "[{}] {}: {}",
                    i + 1,
                    session.technique.as_str(),
                    t.content.chars().take(200).collect::<String>()
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanchallenge::store::HumanChallengeStore;
    use std::sync::Arc;

    #[test]
    fn test_start_session_creates_opening_turn() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let engine = IntrospectionEngine::new(store.clone());

        let session = engine
            .start_session(
                "hc_test_001",
                ChallengeType::Decision,
                "Elegir entre SQLite y PostgreSQL",
                Some(IntrospectionTechnique::PreMortem),
            )
            .unwrap();

        assert!(session.id.starts_with("is_"));
        assert_eq!(session.turns.len(), 1);
        assert_eq!(session.turns[0].role, TurnRole::LlmGuide);
        assert_eq!(session.status, IntrospectionStatus::Active);
    }

    #[test]
    fn test_process_turn_adds_human_and_guide() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let engine = IntrospectionEngine::new(store.clone());

        let session = engine
            .start_session(
                "hc_test_002",
                ChallengeType::Assumption,
                "Asumimos que el puerto 8006 está libre",
                Some(IntrospectionTechnique::SocraticQuestioning),
            )
            .unwrap();

        let updated = engine
            .process_turn(
                &session.id,
                "Asumí que estaba libre porque nadie más lo usa normalmente",
                "Asumimos que el puerto 8006 está libre",
            )
            .unwrap();

        // Opening turn + human + LLM response = 3
        assert_eq!(updated.turns.len(), 3);
        assert_eq!(updated.turns[1].role, TurnRole::Human);
        assert_eq!(updated.turns[2].role, TurnRole::LlmGuide);
        assert!(updated.depth_score > 0.0);
    }

    #[test]
    fn test_five_whys_auto_completes_at_5() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let engine = IntrospectionEngine::new(store.clone());

        let session = engine
            .start_session(
                "hc_test_003",
                ChallengeType::Contradiction,
                "Contradicción en los requisitos de sync",
                Some(IntrospectionTechnique::FiveWhys),
            )
            .unwrap();

        let mut current = session;
        let desc = "Contradicción en los requisitos de sync";
        for i in 0..5 {
            let input = format!(
                "Porque la causa {} es esta razón importante que tiene más de 50 chars para profundidad",
                i
            );
            current = engine.process_turn(&current.id, &input, desc).unwrap();
        }

        assert_eq!(current.status, IntrospectionStatus::Completed);
        assert!(!current.insights.is_empty());
    }

    #[test]
    fn test_recommend_technique_by_challenge_type() {
        assert_eq!(
            IntrospectionTechnique::recommend_for(ChallengeType::Decision),
            IntrospectionTechnique::PreMortem
        );
        assert_eq!(
            IntrospectionTechnique::recommend_for(ChallengeType::Assumption),
            IntrospectionTechnique::SocraticQuestioning
        );
        assert_eq!(
            IntrospectionTechnique::recommend_for(ChallengeType::Contradiction),
            IntrospectionTechnique::SteelManning
        );
    }
}
