use crate::clockify::Token;
use anyhow::Error;
use chrono::NaiveDate;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
};

type CachedDates = HashMap<Token, NaiveDate>;

fn read_cached_dates() -> Result<CachedDates, Error> {
    let path = Path::new(".clockify-rust");
    if path.exists() && path.is_file() {
        let mut file = std::fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bincode::deserialize(bytes.as_slice())?)
    } else {
        Ok(HashMap::new())
    }
}

fn save_cached_dates(dates: HashMap<Token, NaiveDate>) -> Result<(), Error> {
    let path = Path::new(".clockify-rust");
    let bytes = bincode::serialize(&dates)?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes.as_slice())?;
    Ok(())
}

pub(crate) fn set_cache_first_date(token: &Token, date: &NaiveDate) -> Result<(), Error> {
    let mut cached_dates: CachedDates = read_cached_dates()?;
    cached_dates.insert(token.clone(), *date);
    save_cached_dates(cached_dates)?;
    Ok(())
}

pub(crate) fn get_cache_first_date(token: &Token) -> Result<Option<NaiveDate>, Error> {
    let cached_dates: CachedDates = read_cached_dates()?;
    Ok(cached_dates.get(token).copied())
}
