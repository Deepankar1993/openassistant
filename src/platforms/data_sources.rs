// src/platforms/data_sources.rs
use super::DataSource;
use tracing::info;

/// Built-in data source definitions matching OpenHumans integrations
pub fn list_data_sources() -> Vec<DataSource> {
    vec![
        DataSource {
            name: "Google Fit".to_string(),
            description: "Health and fitness data from Google Fit".to_string(),
            url: "https://fit.google.com".to_string(),
            data_type: "health".to_string(),
        },
        DataSource {
            name: "Apple Health".to_string(),
            description: "Health data from Apple HealthKit".to_string(),
            url: "https://www.apple.com/health/".to_string(),
            data_type: "health".to_string(),
        },
        DataSource {
            name: "Fitbit".to_string(),
            description: "Activity and sleep data from Fitbit".to_string(),
            url: "https://www.fitbit.com".to_string(),
            data_type: "health".to_string(),
        },
        DataSource {
            name: "Twitter Archive".to_string(),
            description: "Your Twitter/X data export".to_string(),
            url: "https://twitter.com".to_string(),
            data_type: "social".to_string(),
        },
        DataSource {
            name: "GPS Logger".to_string(),
            description: "Geolocation data".to_string(),
            url: "".to_string(),
            data_type: "location".to_string(),
        },
    ]
}
