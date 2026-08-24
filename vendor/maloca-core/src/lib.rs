pub mod humanchallenge {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Challenge {
        pub id: String,
        pub challenge_type: String,
    }
}

pub mod models {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Model {
        pub id: String,
        pub name: String,
    }
}
