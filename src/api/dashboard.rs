//! Admin dashboard endpoint — totals, charts and breakdowns for the admin UI.

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::{
    AccessTimelinePoint, ContentTypeBreakdown, DashboardFilter, DashboardStats, DashboardTotals,
    StorageBreakdown, SyncStatusPoint, TimelinePoint, TopAccessedFile,
};
use crate::error::AppError;

use super::auth::AdminUser;

const TOP_N: i64 = 10;

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    /// One of "7d" | "30d" | "90d" | "1y". Defaults to "30d".
    pub period: Option<String>,
    pub project_id: Option<Uuid>,
    pub storage_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct DashboardResponse {
    pub period: String,
    pub start: DateTime<Utc>,
    pub bucket: String,
    pub totals: DashboardTotals,
    pub upload_timeline: Vec<TimelinePoint>,
    pub access_timeline: Vec<AccessTimelinePoint>,
    pub by_content_type: Vec<ContentTypeBreakdown>,
    pub by_storage: Vec<StorageBreakdown>,
    pub sync_status_trend: Vec<SyncStatusPoint>,
    pub top_accessed_files: Vec<TopAccessedFile>,
}

/// Resolve a period label to (start timestamp, bucket name, canonical label).
/// Buckets are chosen so the timeline stays under ~60 points: day for ≤90d,
/// week for 1y, month for the all-time view.
fn resolve_period(label: &str) -> (DateTime<Utc>, &'static str, &'static str) {
    let now = Utc::now();
    match label {
        "today" => {
            let start = now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|dt| dt.and_utc())
                .unwrap_or(now);
            (start, "day", "today")
        }
        "7d" => (now - ChronoDuration::days(7), "day", "7d"),
        "90d" => (now - ChronoDuration::days(90), "day", "90d"),
        "1y" => (now - ChronoDuration::days(365), "week", "1y"),
        "all" => (
            DateTime::<Utc>::from_timestamp(0, 0).unwrap_or(now),
            "month",
            "all",
        ),
        _ => (now - ChronoDuration::days(30), "day", "30d"), // default + "30d"
    }
}

async fn dashboard(
    _admin: AdminUser,
    pool: web::Data<PgPool>,
    query: web::Query<DashboardQuery>,
) -> Result<HttpResponse, AppError> {
    let label = query.period.as_deref().unwrap_or("30d");
    let (start, bucket, period_out) = resolve_period(label);
    let filter = DashboardFilter {
        start,
        project_id: query.project_id,
        storage_id: query.storage_id,
    };

    let totals = DashboardStats::totals(pool.get_ref(), filter).await?;
    let upload_timeline = DashboardStats::upload_timeline(pool.get_ref(), filter, bucket).await?;
    let access_timeline = DashboardStats::access_timeline(pool.get_ref(), filter, bucket).await?;
    let by_content_type = DashboardStats::by_content_type(pool.get_ref(), filter, TOP_N).await?;
    let by_storage = DashboardStats::by_storage(pool.get_ref(), filter).await?;
    let sync_status_trend = DashboardStats::sync_status_trend(pool.get_ref(), filter, bucket).await?;
    let top_accessed_files = DashboardStats::top_accessed_files(pool.get_ref(), filter, TOP_N).await?;

    Ok(HttpResponse::Ok().json(DashboardResponse {
        period: period_out.to_string(),
        start,
        bucket: bucket.to_string(),
        totals,
        upload_timeline,
        access_timeline,
        by_content_type,
        by_storage,
        sync_status_trend,
        top_accessed_files,
    }))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/system/dashboard", web::get().to(dashboard));
}
