use chrono::{DateTime, FixedOffset, Utc};
use dotenvy::dotenv;
use rss::Channel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::thread;
use std::time::Duration;

// CONFIGURATION
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
        color: 1791981, // Arch Blue
    },
    FeedConfig {
        url: "https://archlinux.org/feeds/news/",
        color: 13438481, // Orange
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
struct DiscordMessage {
    username: String,
    embeds: Vec<DiscordEmbed>,
}

#[derive(Serialize)]
struct DiscordEmbed {
    title: String,
    url: String,
    description: String,
    color: u32,
    footer: DiscordFooter,
    timestamp: String,
}

#[derive(Serialize)]
struct DiscordFooter {
    text: String,
}

// No #[tokio::main] needed anymore!
fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    let webhook_url = env::var("DISCORD_WEBHOOK_URL")
        .expect("DISCORD_WEBHOOK_URL must be set in .env or environment");

    println!("Arch Linux News Bot started (Lightweight Mode).");
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

        // Standard thread sleep
        thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECONDS));
    }
}

// Removed 'async', removed 'Client' (ureq creates agents on the fly or you can reuse one)
fn fetch_and_process_feed(
    feed_config: &FeedConfig,
    state: &mut AppState,
    webhook_url: &str,
) -> Result<bool, Box<dyn Error>> {
    // ureq blocking call
    let response = ureq::get(feed_config.url).call()?;
    let mut content = Vec::new();
    response.into_reader().read_to_end(&mut content)?;

    let channel = Channel::read_from(&content[..])?;

    // Parse dates
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

    for (item, date) in items_to_send {
        send_discord_webhook(item, &channel.title, feed_config.color, webhook_url)?;

        if date > current_max_date {
            current_max_date = date;
        }
        // Standard thread sleep
        thread::sleep(Duration::from_millis(1000));
    }

    state
        .last_seen
        .insert(feed_config.url.to_string(), current_max_date);
    Ok(true)
}

fn send_discord_webhook(
    item: &rss::Item,
    feed_title: &str,
    color: u32,
    webhook_url: &str,
) -> Result<(), Box<dyn Error>> {
    let payload = DiscordMessage {
        username: "Arch Linux Bot".to_string(),
        embeds: vec![DiscordEmbed {
            title: item.title().unwrap_or("No Title").to_string(),
            url: item.link().unwrap_or("").to_string(),
            description: item
                .description()
                .map(|d| {
                    if d.len() > 200 {
                        format!("{}...", &d[0..200])
                    } else {
                        d.to_string()
                    }
                })
                .unwrap_or_else(|| "No description".to_string()),
            color,
            footer: DiscordFooter {
                text: format!("Source: {}", feed_title),
            },
            timestamp: Utc::now().to_rfc3339(),
        }],
    };

    let body = serde_json::to_vec(&payload)?;
    ureq::post(webhook_url)
        .set("Content-Type", "application/json")
        .send_bytes(&body)?;

    println!("Sent update: {}", item.title().unwrap_or("Unknown"));
    Ok(())
}
