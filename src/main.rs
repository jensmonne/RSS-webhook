use chrono::{DateTime, FixedOffset, Utc};
use reqwest::Client;
use rss::Channel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::Duration;
use tokio::time::sleep;

// CONFIGURATION
const CHECK_INTERVAL_SECONDS: u64 = 300; // Check every 5 minutes
const STATE_FILE: &str = "state.json";
const DISCORD_WEBHOOK_URL: &str = "https://discord.com/api/webhooks/1467986078387671183/onsmT6nJ2pIGhzcgUyhzVG_nWdx0Is5ipG5Ava3WYBXAm87scR8M8qS3ZepPx2zT9GW3";

#[derive(Debug)]
struct FeedConfig {
    url: &'static str,
    color: u32,
}

const FEEDS: &[FeedConfig] = &[
    FeedConfig {
        url: "https://archlinux.org/feeds/packages/x86_64/core/",
        color: 1791981, // Arch Blue (#1793D1)
    },
    FeedConfig {
        url: "https://archlinux.org/feeds/news/",
        color: 13438481, // Orange (#E67E22) to make news stand out
    },
];

// Data structure to save the last seen timestamp for each feed
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

// Structure for Discord Webhook Payload
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let client = Client::new();

    println!("Arch Linux News Bot started.");
    println!("Monitoring feeds: {:?}", FEEDS);

    loop {
        let mut state = AppState::load();
        let mut state_changed = false;

        // Iterate over the Config structs now, not just strings
        for feed in FEEDS {
            match fetch_and_process_feed(&client, feed, &mut state).await {
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

        sleep(Duration::from_secs(CHECK_INTERVAL_SECONDS)).await;
    }
}

// Updated to accept &FeedConfig instead of just &str
async fn fetch_and_process_feed(
    client: &Client,
    feed_config: &FeedConfig,
    state: &mut AppState,
) -> Result<bool, Box<dyn Error>> {
    let content = client.get(feed_config.url).send().await?.bytes().await?;
    let channel = Channel::read_from(&content[..])?;

    // Default start date (Update this if you want to skip old items)
    let last_seen_date = *state
        .last_seen
        .get(feed_config.url)
        .unwrap_or(&DateTime::parse_from_rfc3339("2025-10-01T00:00:00+00:00")?);

    let mut new_max_date = last_seen_date;
    let mut sent_update = false;

    for item in channel.items().iter().rev() {
        if let Some(pub_date_str) = item.pub_date() {
            let item_date = DateTime::parse_from_rfc2822(pub_date_str)?;

            if item_date > last_seen_date {
                // Pass the color from the config to the sender function
                send_discord_webhook(client, item, &channel.title, feed_config.color).await?;

                if item_date > new_max_date {
                    new_max_date = item_date;
                }
                sent_update = true;
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    if sent_update {
        state
            .last_seen
            .insert(feed_config.url.to_string(), new_max_date);
    }

    Ok(sent_update)
}

async fn send_discord_webhook(
    client: &Client,
    item: &rss::Item,
    feed_title: &str,
    color: u32, // Added color parameter
) -> Result<(), Box<dyn Error>> {
    let title = item.title().unwrap_or("No Title").to_string();
    let link = item.link().unwrap_or("").to_string();
    let description = item.description().unwrap_or("No description").to_string();

    let clean_desc = if description.len() > 200 {
        format!("{}...", &description[0..200])
    } else {
        description
    };

    let payload = DiscordMessage {
        username: "Arch Linux Bot".to_string(),
        embeds: vec![DiscordEmbed {
            title,
            url: link,
            description: clean_desc,
            color, // Use the dynamic color passed in
            footer: DiscordFooter {
                text: format!("Source: {}", feed_title),
            },
            timestamp: Utc::now().to_rfc3339(),
        }],
    };

    let res = client
        .post(DISCORD_WEBHOOK_URL)
        .json(&payload)
        .send()
        .await?;

    if !res.status().is_success() {
        eprintln!("Failed to send webhook: Status {}", res.status());
    } else {
        println!("Sent update: {}", item.title().unwrap_or("Unknown"));
    }

    Ok(())
}
