use axum::{
    extract::{Extension, Json, Query},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Weekday};
use reqwest;
use serde::Deserialize;
use sqlx::sqlite::SqlitePool;
use std::{collections::HashMap, net::SocketAddr};
use tokio;

#[derive(Debug, Deserialize)]
struct StravaEvent {
    aspect_type: String,
    event_time: u64,
    object_id: u64,
    object_type: String, // Expect "activity"
    owner_id: u64,
    subscription_id: u64,
    updates: Option<serde_json::Value>,
}

// Representation of an Activity fetched from Strava API.
#[derive(Debug, Deserialize)]
struct Activity {
    id: u64,
    name: String,
    #[serde(rename = "type")]
    activity_type: String, // e.g., "Walk"
    start_date_local: String, // ISO 8601 format
    start_latlng: Option<Vec<f64>>, // [latitude, longitude]
    // Additional fields can be added as needed
}

/// Process an activity by:
/// - Ensuring it hasn't been processed before.
/// - Fetching its full details from Strava.
/// - Checking if it’s a weekday walk in our target geofence.
/// - Updating it to be "private" with the name "Rusty".
/// - Recording it in the SQLite database.
async fn process_activity(
    activity_id: u64,
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if the activity was already processed.
    let rec = sqlx::query!(
        "SELECT id FROM processed_activities WHERE activity_id = ?",
        activity_id as i64
    )
    .fetch_optional(&pool)
    .await?;
    if rec.is_some() {
        println!("Activity {} already processed.", activity_id);
        return Ok(());
    }

    // Replace with your actual access token.
    let access_token = "YOUR_ACCESS_TOKEN";
    let client = reqwest::Client::new();
    let activity_url = format!("https://www.strava.com/api/v3/activities/{}", activity_id);
    let res = client
        .get(&activity_url)
        .bearer_auth(access_token)
        .send()
        .await?;
    let activity: Activity = res.json().await?;

    // Filter: Process only if the activity is a "walk" (case insensitive).
    if activity.activity_type.to_lowercase() != "walk" {
        println!("Activity {} is not a walk.", activity_id);
        return Ok(());
    }

    // Parse the local start date and check if it's a weekday.
    let dt = DateTime::parse_from_rfc3339(&activity.start_date_local)?;
    if matches!(dt.weekday(), Weekday::Sat | Weekday::Sun) {
        println!("Activity {} occurred on a weekend.", activity_id);
        return Ok(());
    }

    // Define your geofence (example: latitude between 40.0 and 41.0,
    // longitude between -74.0 and -73.0).
    if let Some(coords) = activity.start_latlng {
        let (lat, lng) = (coords[0], coords[1]);
        if !(lat >= 40.0 && lat <= 41.0 && lng >= -74.0 && lng <= -73.0) {
            println!("Activity {} is not in the specified location.", activity_id);
            return Ok(());
        }
    } else {
        println!("Activity {} has no location data.", activity_id);
        return Ok(());
    }

    // All conditions met: update the activity.
    // Strava uses the "private" parameter (1 for "only you", 0 for public).
    let update_url = format!("https://www.strava.com/api/v3/activities/{}", activity_id);
    let params = [("name", "Rusty"), ("private", "1")];
    let update_res = client
        .put(&update_url)
        .bearer_auth(access_token)
        .form(&params)
        .send()
        .await?;
    if update_res.status().is_success() {
        println!("Activity {} updated successfully.", activity_id);
        // Record the processed activity.
        sqlx::query!(
            "INSERT INTO processed_activities (activity_id) VALUES (?)",
            activity_id as i64
        )
        .execute(&pool)
        .await?;
    } else {
        println!(
            "Failed to update activity {}. Status: {}",
            activity_id,
            update_res.status()
        );
    }

    Ok(())
}

/// The webhook handler serves two purposes:
/// - GET: Handles the verification challenge from Strava (respond with the challenge).
/// - POST: Receives activity events.
async fn webhook_handler(
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<serde_json::Value>,
    Extension(pool): Extension<SqlitePool>,
) -> impl axum::response::IntoResponse {
    // If a challenge parameter is present (Strava verification), respond immediately.
    if let Some(challenge) = params.get("hub.challenge") {
        return challenge.to_string();
    }

    // Parse the webhook POST payload.
    if let Ok(event) = serde_json::from_value::<StravaEvent>(payload.clone()) {
        if event.object_type == "activity" {
            let pool_clone = pool.clone();
            // Process the event asynchronously.
            tokio::spawn(async move {
                if let Err(e) = process_activity(event.object_id, pool_clone).await {
                    eprintln!("Error processing activity {}: {:?}", event.object_id, e);
                }
            });
        }
    }
    "OK"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a SQLite connection pool.
    let pool = SqlitePool::connect("sqlite://processed_activities.db").await?;

    // Run a migration to create the table if it doesn't exist.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS processed_activities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            activity_id INTEGER NOT NULL UNIQUE,
            processed_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    // Build the axum application.
    let app = Router::new()
        .route("/webhook", get(webhook_handler).post(webhook_handler))
        .layer(Extension(pool));

    // Start the server on port 3000.
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
