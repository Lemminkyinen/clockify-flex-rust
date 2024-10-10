pub(crate) mod schema;

use anyhow::Error;
use schema::ExtraSettings;
use tokio::{fs::File, io::AsyncReadExt};

pub(crate) struct GlobalSettings {
    settings: Vec<ExtraSettings>,
}

impl GlobalSettings {
    async fn read_extra_settings() -> Result<Vec<ExtraSettings>, Error> {
        let mut settings = File::open(".settings.json").await?;
        let mut json = String::new();
        settings.read_to_string(&mut json).await?;
        let data: Vec<ExtraSettings> = serde_json::from_str(&json)?;
        Ok(data)
    }

    pub(crate) async fn create_settings() -> GlobalSettings {
        let settings = match Self::read_extra_settings().await {
            Ok(settings) => settings,
            Err(err) => {
                println!("Could not read extra settings: {}", err);
                Vec::with_capacity(0)
            }
        };
        Self { settings }
    }

    pub(crate) fn get_user_settings(&self, email: &str) -> Option<ExtraSettings> {
        self.settings
            .iter()
            .find(|single_settings| single_settings.email == email)
            .cloned()
    }
}
