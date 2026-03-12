use actix_web::{web, HttpResponse};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::{CreateProject, Project, UpdateProject};
use crate::error::AppError;

async fn create_project(
    pool: web::Data<PgPool>,
    body: web::Json<CreateProject>,
) -> Result<HttpResponse, AppError> {
    let project = Project::create(pool.get_ref(), &body).await?;
    Ok(HttpResponse::Created().json(project))
}

async fn list_projects(pool: web::Data<PgPool>) -> Result<HttpResponse, AppError> {
    let projects = Project::list(pool.get_ref()).await?;
    Ok(HttpResponse::Ok().json(projects))
}

async fn get_project(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;

    // Get file stats for the project
    let stats_row = sqlx::query(
        r#"SELECT COUNT(*)::bigint as file_count,
                  COALESCE(SUM(size), 0)::bigint as total_size
           FROM (
               SELECT DISTINCT f.id, f.size
               FROM file_references fr
               JOIN files f ON f.id = fr.file_id
               WHERE fr.project_id = $1
           ) sub"#,
    )
    .bind(project_id)
    .fetch_one(pool.get_ref())
    .await?;

    let file_count: i64 = sqlx::Row::get(&stats_row, 0);
    let total_size: i64 = sqlx::Row::get(&stats_row, 1);

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "project": project,
        "stats": {
            "file_count": file_count,
            "total_size": total_size,
        }
    })))
}

async fn update_project(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateProject>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let project = Project::update(pool.get_ref(), project_id, &body).await?;
    Ok(HttpResponse::Ok().json(project))
}

async fn delete_project(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    Project::delete(pool.get_ref(), project_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/projects")
            .route(web::post().to(create_project))
            .route(web::get().to(list_projects)),
    )
    .service(
        web::resource("/projects/{id}")
            .route(web::get().to(get_project))
            .route(web::put().to(update_project))
            .route(web::delete().to(delete_project)),
    );
}

#[cfg(test)]
mod tests {
    use crate::db::models::{CreateProject, UpdateProject};

    #[test]
    fn test_create_project_deserialization() {
        let json = serde_json::json!({
            "name": "My Project",
            "slug": "my-project",
            "hot_to_cold_days": 30
        });
        let input: CreateProject = serde_json::from_value(json).unwrap();
        assert_eq!(input.name, "My Project");
        assert_eq!(input.slug, "my-project");
        assert_eq!(input.hot_to_cold_days, Some(30));
    }

    #[test]
    fn test_create_project_without_optional_fields() {
        let json = serde_json::json!({
            "name": "My Project",
            "slug": "my-project"
        });
        let input: CreateProject = serde_json::from_value(json).unwrap();
        assert!(input.hot_to_cold_days.is_none());
    }

    #[test]
    fn test_update_project_partial() {
        let json = serde_json::json!({
            "name": "Updated Name"
        });
        let input: UpdateProject = serde_json::from_value(json).unwrap();
        assert_eq!(input.name, Some("Updated Name".to_string()));
        assert!(input.slug.is_none());
        assert!(input.hot_to_cold_days.is_none());
    }

    #[test]
    fn test_update_project_set_hot_to_cold() {
        let json = serde_json::json!({
            "hot_to_cold_days": 14
        });
        let input: UpdateProject = serde_json::from_value(json).unwrap();
        assert_eq!(input.hot_to_cold_days, Some(Some(14)));
    }

    #[test]
    fn test_create_project_missing_required_fields() {
        let json = serde_json::json!({
            "name": "Only Name"
        });
        let result: Result<CreateProject, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }
}
