use crate::args::get_settings;
use crate::models::{Day, Holiday, HolidayType, SickLeaveDay, WorkDay, WorkItem};
use crate::utils::{self, json_to_disk};
use anyhow::Error;
use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, TimeZone, Utc};
use futures::future::join_all;
use itertools::Itertools;
use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::time::Duration;
use std::{fmt, thread, time};
use tokio::time::sleep;
use url::Url;

lazy_static! {
    static ref API_URL: Url =
        Url::parse("https://global.api.clockify.me/").expect("Cannot parse clockify url!");
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, PartialOrd, Hash)]
pub(crate) struct Token(String);

impl Token {
    pub(crate) fn new(token: &str) -> Self {
        Token(token.to_owned())
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&Token> for HeaderValue {
    fn from(val: &Token) -> Self {
        HeaderValue::from_str(&val.0)
            .unwrap_or_else(|_| panic!("Failed to transform '{}' to header!", val.0))
    }
}

impl From<&str> for Token {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

fn str_hex_to_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    u128::from_str_radix(&s, 16).map_err(serde::de::Error::custom)
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct User {
    #[serde(deserialize_with = "str_hex_to_u128")]
    id: u128,
    #[serde(rename(deserialize = "activeWorkspace"))]
    #[serde(deserialize_with = "str_hex_to_u128")]
    workspace: u128,
    name: String,
    pub(crate) email: String,
}

impl User {
    fn workspace_str(&self) -> String {
        format!("{:x}", self.workspace)
    }

    fn id_str(&self) -> String {
        format!("{:x}", self.id)
    }
}

async fn get_user(client: Client, token: &Token) -> Result<User, Error> {
    let user_url = API_URL.join("v1/user")?;
    let response = client
        .get(user_url)
        .header("x-api-key", token)
        .send()
        .await?;
    response.json::<User>().await.map_err(Error::from)
}

fn get_string_field<E: serde::de::Error>(obj: &Value, field: &'static str) -> Result<String, E> {
    obj.get(field)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| E::missing_field(field))
}

fn get_datetime_field<E: serde::de::Error>(
    obj: &Value,
    field: &'static str,
) -> Result<DateTime<Utc>, E> {
    obj.get(field)
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| E::missing_field(field))
}

#[derive(Debug, Serialize)]
pub(crate) struct TimeEntry {
    pub description: String,
    pub project_name: String,
    pub user_id: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for TimeEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v: Value = Deserialize::deserialize(deserializer)?;

        let description = get_string_field(&v, "description")?;

        let project = v
            .get("project")
            .ok_or_else(|| serde::de::Error::missing_field("project"))?;
        let project_name = get_string_field(project, "name")?;

        let user = v
            .get("user")
            .ok_or_else(|| serde::de::Error::missing_field("user"))?;
        let user_id = get_string_field(user, "id")?;

        let time_interval = v
            .get("timeInterval")
            .ok_or_else(|| serde::de::Error::missing_field("timeInterval"))?;
        let start = get_datetime_field(time_interval, "start")?;
        let end = get_datetime_field(time_interval, "end")?;

        Ok(TimeEntry {
            description,
            project_name,
            user_id,
            start,
            end,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum TimeOffType {
    DayOff,
    SickLeave,
    Vacation,
    ParentalLeave,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TimeOffItem {
    pub note: String,
    pub user_id: String,
    pub type_: TimeOffType,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub status: String,
}

impl<'de> Deserialize<'de> for TimeOffItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v: Value = Deserialize::deserialize(deserializer)?;

        // Assert that time unit is days
        let time_unit = get_string_field(&v, "timeUnit")?;
        assert!(&time_unit == "DAYS", "Time unit wasn't 'DAYS'! {time_unit}");

        let user_id = get_string_field(&v, "userId")?;
        let policy_name = get_string_field(&v, "policyName")?;
        let type_ = match policy_name.as_str() {
            "Day off" => TimeOffType::DayOff,
            "Sick leave" => TimeOffType::SickLeave,
            "Vacation" => TimeOffType::Vacation,
            "Parental leave" => TimeOffType::ParentalLeave,
            x => return Err(serde::de::Error::custom(format!("unknown policyName: {x}"))),
        };

        let status_object = v
            .get("status")
            .ok_or_else(|| serde::de::Error::missing_field("status"))?;
        let status = get_string_field(status_object, "statusType")?;

        let note = get_string_field::<D::Error>(&v, "note").unwrap_or_default();

        let time_off_object = v
            .get("timeOffPeriod")
            .ok_or_else(|| serde::de::Error::missing_field("timeOffPeriod"))?;
        let period = time_off_object
            .get("period")
            .ok_or_else(|| serde::de::Error::missing_field("period"))?;
        let start = get_datetime_field(period, "start")?;
        let end = get_datetime_field(period, "end")?;

        Ok(TimeOffItem {
            note,
            type_,
            user_id,
            start,
            end,
            status,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ClockifyClient {
    base_url: &'static Url,
    pub(crate) user: User,
    client: Client,
}

impl ClockifyClient {
    pub(crate) fn new(token: &Token) -> Result<Self, Error> {
        let token_ = &token.clone();

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", token_.into());
        let client = Client::builder().default_headers(headers).build()?;
        let client_ = client.clone();

        let user = tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(async move {
                let mut attempts = 0u8;
                loop {
                    match get_user(client_.clone(), token_).await {
                        Ok(user) => return Ok(user),
                        Err(e) if attempts < 3 => {
                            log::error!("Failed to get user from clockify API: {e}");
                            attempts += 1;
                            sleep(Duration::from_secs(2)).await;
                        }
                        Err(e) => {
                            log::error!("Failed to get user from clockify API three times: {e}");
                            return Err(e);
                        }
                    }
                }
            })
        })?;

        Ok(ClockifyClient {
            user,
            base_url: &API_URL,
            client,
        })
    }

    pub(crate) async fn get_work_items_since(
        &self,
        date: &NaiveDate,
    ) -> Result<Vec<TimeEntry>, Error> {
        let time_entries_path = format!(
            "workspaces/{}/timeEntries/users/{}/timesheet",
            self.user.workspace_str(),
            self.user.id_str()
        );
        let url = self.base_url.join(&time_entries_path)?;

        // Default is end of today
        let end = Utc::now().date_naive().and_time(
            NaiveTime::from_hms_opt(23, 59, 59).ok_or(Error::msg("Cannot create NaiveTime"))?,
        );
        let end = Utc.from_utc_datetime(&end);

        let start = date.and_time(NaiveTime::MIN);
        let start = Utc.from_utc_datetime(&start);

        // The clockify API limits queries to 999 hours (approx. 41.625 days)
        let mut queries = Vec::new();
        let mut current_start = start;

        while current_start < end {
            let current_end = std::cmp::min(
                current_start
                    .checked_add_signed(TimeDelta::days(41))
                    .unwrap_or(end),
                end,
            );

            queries.push((current_start, current_end));
            current_start = current_end;
        }

        let mut request_futures = queries
            .into_iter()
            .map(|(start, end)| {
                self.client
                    .get(url.clone())
                    .query(&[
                        (
                            "start",
                            start.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        ),
                        (
                            "end",
                            end.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        ),
                        ("in-progress", false.to_string()),
                        ("page", 0.to_string()),
                        ("page-size", 0.to_string()),
                    ])
                    .send()
            })
            .collect_vec();

        let mut responses = Vec::with_capacity(request_futures.len());
        let request_chunks = request_futures.chunks_mut(18);

        '_chunk: for chunk in request_chunks {
            'result: for result in join_all(chunk).await {
                match result {
                    Err(e) => return Err(e.into()),
                    Ok(res) => {
                        if !res.status().is_success() {
                            println!("Unsuccessful response!");
                            continue 'result;
                        }

                        let headers = res.headers();
                        let cont_len = headers
                            .get("content-length")
                            .map(|v| v.to_str().map_err(Error::msg));
                        let transfer_encoding = headers
                            .get("transfer-encoding")
                            .map(|v| v.to_str().map_err(Error::msg));

                        if let Some(Ok(encoding)) = transfer_encoding {
                            if encoding == "chunked" {
                                responses.push(res);
                                continue 'result;
                            }
                        }
                        if let Some(Ok(cont_len)) = cont_len {
                            if !["0", "2"].contains(&cont_len) {
                                responses.push(res);
                            }
                        }
                    }
                };
            }
            if responses.capacity() > 18 {
                thread::sleep(time::Duration::from_millis(1000));
            }
        }

        let json_futures = responses
            .into_iter()
            .map(|response| response.json::<Vec<TimeEntry>>());

        let mut jsons = Vec::with_capacity(json_futures.len());
        for result in join_all(json_futures).await {
            if let Err(e) = result {
                return Err(e.into());
            };
            jsons.push(result.unwrap())
        }

        if get_settings().await.debug {
            let path = format!("work_items_{}.json", Utc::now().format("%Y%m%d%H%M%S"));
            if let Err(e) = json_to_disk(path, &jsons).await {
                println!("Failed to save work items to disk! {e}")
            };
        }

        Ok(jsons.into_iter().flatten().collect())
    }

    pub(crate) async fn get_time_off_items(&self) -> Result<Vec<TimeOffItem>, Error> {
        let time_entries_path =
            format!("workspaces/{}/time-off/requests", self.user.workspace_str());
        let url = self.base_url.join(&time_entries_path)?;

        let body = &serde_json::json!({
            "page": 1,
            "pageSize": 500,
            "status": ["APPROVED"],
            "users": {
                "contains": "CONTAINS",
                "ids": [self.user.id_str()],
                "status": "ALL"
            },
            "userGroups": {}
        });

        let mut response = self.client.post(url.clone()).json(body).send().await?;
        if response.status().as_u16() == 429 {
            'cooldown: for x in [600, 750, 1250, 2000] {
                // Clockify is rate limiting.. Cooling down a bit and trying again... ({} ms)
                std::thread::sleep(std::time::Duration::from_millis(x));
                response = self.client.post(url.clone()).json(body).send().await?;
                if response.status().is_success() {
                    break 'cooldown;
                }
            }
        }
        let response_json = response.json::<Value>().await?;
        let count = response_json
            .get("count")
            .ok_or_else(|| Error::msg("missing count"))?
            .as_u64()
            .ok_or_else(|| Error::msg("count is not a number"))? as usize;
        let time_off_item_results = response_json
            .get("requests")
            .ok_or(Error::msg("requests not found"))?
            .as_array()
            .ok_or(Error::msg("Array couldn't be formed!"))?
            .iter()
            .cloned()
            .map(serde_json::from_value::<TimeOffItem>);

        let mut time_off_items = Vec::with_capacity(count);
        for result in time_off_item_results {
            match result {
                Ok(result) => time_off_items.push(result),
                Err(err) => return Err(err.into()),
            }
        }

        if get_settings().await.debug {
            let path = format!("time_off_items_{}.json", Utc::now().format("%Y%m%d%H%M%S"));
            if let Err(e) = json_to_disk(path, &time_off_items).await {
                println!("Failed to time off items to disk! {e}")
            };
        }

        Ok(time_off_items)
    }
}

pub(crate) async fn get_working_days(
    client: ClockifyClient,
    since: &NaiveDate,
) -> Result<Vec<WorkDay>, Error> {
    let work_items = client.get_work_items_since(since).await?;
    let work_days = work_items
        .into_iter()
        .chunk_by(|wi| wi.start.date_naive())
        .into_iter()
        .map(|(date, group)| {
            let work_items = group.map(WorkItem::from).collect();
            WorkDay::new(date, work_items)
        })
        .collect::<Vec<WorkDay>>();

    Ok(work_days)
}

pub(crate) async fn get_days_off(
    client: ClockifyClient,
    since: &NaiveDate,
) -> Result<Vec<Day>, Error> {
    let items = client.get_time_off_items().await?;
    let days_off = items
        .into_iter()
        .flat_map(|toi| {
            // TODO support users datetime
            // Use date_naive because:
            // "start": "2024-01-30T22:00:00Z",
            // "end": "2024-01-31T21:59:59.999Z"
            let start = toi.start.date_naive();
            let end = toi.end.date_naive();
            let mut days_off = Vec::new();
            for date in utils::DateRange(start + TimeDelta::days(1), end).filter(|d| d >= since) {
                let note = toi.note.clone();
                let day_off = match toi.type_ {
                    TimeOffType::SickLeave => {
                        let day = SickLeaveDay::new(note, date);
                        Day::Sick(day)
                    }
                    TimeOffType::Vacation => {
                        let day = Holiday::new(String::new(), date, HolidayType::Vacation);
                        Day::Holiday(day)
                    }
                    TimeOffType::ParentalLeave => {
                        let day = Holiday::new(String::new(), date, HolidayType::ParentalLeave);
                        Day::Holiday(day)
                    }
                    TimeOffType::DayOff => {
                        let day = Holiday::new(String::new(), date, HolidayType::Flex);
                        Day::Holiday(day)
                    }
                };
                days_off.push(day_off);
            }
            days_off
        })
        .collect::<Vec<Day>>();
    Ok(days_off)
}
