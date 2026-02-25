use chrono::{DateTime, FixedOffset};
use rss::Channel;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::thread;
use std::time::Duration;

const CHECK_INTERVAL_SECONDS: u64 = 300;
const STATE_FILE: &str = "state.json";

#[derive(Debug)]
struct FeedConfig {
    url: &'static str,
    color: u32,
}

const FEEDS: &[FeedConfig] = &[
    FeedConfig {
        url: "https://archlinux.org/feeds/packages/x86_64/core/",
        color: 1791981, // Blue
    },
    FeedConfig {
        url: "https://archlinux.org/feeds/news/",
        color: 13438481, // Red
    },
];

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppState {
    last_seen: HashMap<String, DateTime<FixedOffset>>,
}

impl AppState {
    fn new() -> Self {
        AppState {
            last_seen: HashMap::new(),
        }
    }

    fn load() -> Self {
        if let Ok(data) = fs::read_to_string(STATE_FILE) {
            serde_json::from_str(&data).unwrap_or_else(|_| AppState::new())
        } else {
            AppState::new()
        }
    }

    fn save(&self) {
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(STATE_FILE, data);
        }
    }
}

#[derive(Serialize)]
struct DiscordMessage<'a> {
    username: Cow<'a, str>,
    embeds: Vec<DiscordEmbed<'a>>,
}

#[derive(Serialize)]
struct DiscordEmbed<'a> {
    title: Cow<'a, str>,
    url: Cow<'a, str>,
    description: Cow<'a, str>,
    color: u32,
    footer: DiscordFooter<'a>,
    timestamp: String,
}

#[derive(Serialize)]
struct DiscordFooter<'a> {
    text: Cow<'a, str>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let webhook_url = env::var("DISCORD_WEBHOOK_URL")
        .expect("DISCORD_WEBHOOK_URL must be set in .env or environment");

    println!("RSS webhook started.");
    println!("Monitoring feeds: {:?}", FEEDS);

    loop {
        let mut state = AppState::load();
        let mut state_changed = false;

        for feed in FEEDS {
            match fetch_and_process_feed(feed, &mut state, &webhook_url) {
                Ok(updated) => {
                    if updated {
                        state_changed = true;
                    }
                }
                Err(e) => eprintln!("Error processing {}: {}", feed.url, e),
            }
        }

        if state_changed {
            state.save();
        }

        thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECONDS));
    }
}

fn fetch_and_process_feed(
    feed_config: &FeedConfig,
    state: &mut AppState,
    webhook_url: &str,
) -> Result<bool, Box<dyn Error>> {
    let response = ureq::get(feed_config.url).call()?;
    let mut content = Vec::new();
    response.into_reader().read_to_end(&mut content)?;

    let channel = Channel::read_from(&content[..])?;

    let mut items_with_dates: Vec<(&rss::Item, DateTime<FixedOffset>)> = Vec::new();
    for item in channel.items() {
        if let Some(pub_date_str) = item.pub_date() {
            if let Ok(date) = DateTime::parse_from_rfc2822(pub_date_str) {
                items_with_dates.push((item, date));
            }
        }
    }

    let last_seen_option = state.last_seen.get(feed_config.url);

    let items_to_send = match last_seen_option {
        Some(&last_seen) => {
            let mut newer: Vec<_> = items_with_dates
                .into_iter()
                .filter(|(_, d)| *d > last_seen)
                .collect();
            newer.sort_by_key(|(_, d)| *d);
            newer
        }
        None => {
            items_with_dates.sort_by(|a, b| b.1.cmp(&a.1));
            let mut top_3: Vec<_> = items_with_dates.into_iter().take(3).collect();
            top_3.reverse();
            top_3
        }
    };

    if items_to_send.is_empty() {
        return Ok(false);
    }

    let mut current_max_date = last_seen_option.cloned().unwrap_or(items_to_send[0].1);

    for chunk in items_to_send.chunks(10) {
        send_discord_batch(chunk, &channel.title, feed_config.color, webhook_url)?;

        if let Some((_, date)) = chunk.last() {
            if *date > current_max_date {
                current_max_date = *date;
            }
        }

        thread::sleep(Duration::from_millis(1000));
    }

    state
        .last_seen
        .insert(feed_config.url.to_string(), current_max_date);
    Ok(true)
}

fn send_discord_batch(
    items: &[(&rss::Item, DateTime<FixedOffset>)],
    feed_title: &str,
    color: u32,
    webhook_url: &str,
) -> Result<(), Box<dyn Error>> {
    let mut embeds = Vec::new();

    for (item, date) in items {
        let title = item.title().unwrap_or("No Title");
        let link = item.link().unwrap_or("");
        let raw_desc = item.description().unwrap_or("No description");

        let mut cleaned_desc = String::with_capacity(raw_desc.len());
        let mut inside_tag = false;
        for c in raw_desc.chars() {
            match c {
                '<' => inside_tag = true,
                '>' => inside_tag = false,
                _ if !inside_tag => cleaned_desc.push(c),
                _ => {}
            }
        }

        let final_desc = cleaned_desc.replace("&nbsp;", " ").trim().to_string();
        let description = if final_desc.len() > 200 {
            Cow::Owned(format!("{}...", &final_desc[0..200]))
        } else {
            Cow::Owned(final_desc)
        };

        embeds.push(DiscordEmbed {
            title: Cow::Borrowed(title),
            url: Cow::Borrowed(link),
            description,
            color,
            footer: DiscordFooter {
                text: Cow::Owned(format!("{}", feed_title)),
            },
            timestamp: date.to_rfc3339(),
        });
    }

    let payload = DiscordMessage {
        username: Cow::Borrowed("Arch Linux Bot"),
        embeds,
    };

    let body = serde_json::to_vec(&payload)?;
    ureq::post(webhook_url)
        .set("Content-Type", "application/json")
        .send_bytes(&body)?;

    println!("Sent batch of {} updates for {}", items.len(), feed_title);
    Ok(())
}
