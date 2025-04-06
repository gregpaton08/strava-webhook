# strava-webhook

## Create the Database

```bash
sqlite3 processed_activities.db <<EOF
CREATE TABLE IF NOT EXISTS processed_activities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id INTEGER UNIQUE NOT NULL,
    processed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
EOF
```
