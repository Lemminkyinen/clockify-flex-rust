pub(crate) mod schema;

use anyhow::Error;
use schema::ExtraSettings;
use tokio::{fs::metadata, fs::File, io::AsyncReadExt};

pub(crate) struct GlobalSettings {
    settings: Vec<ExtraSettings>,
}

impl GlobalSettings {
    async fn read_extra_settings() -> Result<Option<Vec<ExtraSettings>>, Error> {
        let path = ".settings.json";
        if metadata(path).await.is_err() {
            println!("Extra settings file doesn't exist.");
            return Ok(None);
        }
        let mut settings = File::open(path).await?;
        let mut json = String::new();
        settings.read_to_string(&mut json).await?;
        let data: Vec<ExtraSettings> = serde_json::from_str(&json)?;
        Ok(Some(data))
    }

    pub(crate) async fn create_settings() -> Result<GlobalSettings, Error> {
        let settings = match Self::read_extra_settings().await? {
            Some(settings) => settings,
            None => Vec::with_capacity(0),
        };
        Ok(Self { settings })
    }

    pub(crate) fn get_user_settings(&self, email: &str) -> Option<ExtraSettings> {
        self.settings
            .iter()
            .find(|single_settings| single_settings.email == email)
            .cloned()
    }
}
