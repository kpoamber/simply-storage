use actix_web::{web, HttpResponse};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::models::{
    CreateProject, CreateProjectStorage, Project, ProjectStorage, Storage, UpdateProject,
    UpdateProjectStorage, User, UserProject,
};
use crate::error::AppError;

use super::auth::AuthenticatedUser;

async fn create_project(
    pool: web::Data<PgPool>,
    user: AuthenticatedUser,
    body: web::Json<CreateProject>,
) -> Result<HttpResponse, AppError> {
    let project = Project::create(pool.get_ref(), &body, Some(user.user_id)).await?;
    Ok(HttpResponse::Created().json(project))
}

async fn list_projects(
    pool: web::Data<PgPool>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let projects = if user.is_admin() {
        Project::list(pool.get_ref()).await?
    } else {
        Project::list_accessible(pool.get_ref(), user.user_id).await?
    };
    Ok(HttpResponse::Ok().json(projects))
}

async fn get_project(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    // Read access: admin, owner, or member
    if !user.is_admin() && !user.is_owner(project.owner_id) {
        let is_member = UserProject::is_member(pool.get_ref(), user.user_id, project_id).await?;
        if !is_member {
            return Err(AppError::Forbidden("Access denied: not a member".to_string()));
        }
    }

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
    user: AuthenticatedUser,
    body: web::Json<UpdateProject>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let existing = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(existing.owner_id)?;

    let project = Project::update(pool.get_ref(), project_id, &body).await?;
    Ok(HttpResponse::Ok().json(project))
}

async fn delete_project(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let existing = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(existing.owner_id)?;

    Project::delete(pool.get_ref(), project_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ─── Project-Storage assignment endpoints ────────────────────────────────────

async fn list_project_storages(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    // Read access: admin, owner, or member
    if !user.is_admin() && !user.is_owner(project.owner_id) {
        let is_member = UserProject::is_member(pool.get_ref(), user.user_id, project_id).await?;
        if !is_member {
            return Err(AppError::Forbidden("Access denied: not a member".to_string()));
        }
    }

    let assignments = ProjectStorage::list_for_project(pool.get_ref(), project_id).await?;
    Ok(HttpResponse::Ok().json(assignments))
}

async fn assign_storage(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    body: web::Json<CreateProjectStorage>,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;
    Storage::find_by_id(pool.get_ref(), body.storage_id).await?;

    let assignment = ProjectStorage::create(pool.get_ref(), project_id, &body).await?;
    Ok(HttpResponse::Created().json(assignment))
}

async fn update_project_storage(
    pool: web::Data<PgPool>,
    path: web::Path<(Uuid, Uuid)>,
    user: AuthenticatedUser,
    body: web::Json<UpdateProjectStorage>,
) -> Result<HttpResponse, AppError> {
    let (project_id, storage_id) = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    let assignment =
        ProjectStorage::update(pool.get_ref(), project_id, storage_id, &body).await?;
    Ok(HttpResponse::Ok().json(assignment))
}

async fn remove_storage_assignment(
    pool: web::Data<PgPool>,
    path: web::Path<(Uuid, Uuid)>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let (project_id, storage_id) = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    ProjectStorage::delete(pool.get_ref(), project_id, storage_id).await?;
    Ok(HttpResponse::NoContent().finish())
}

async fn list_available_storages(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let project_id = path.into_inner();
    let project = Project::find_by_id(pool.get_ref(), project_id).await?;
    user.require_owner_or_admin(project.owner_id)?;

    let storages = ProjectStorage::list_available_storages(pool.get_ref(), project_id).await?;
    // Redact sensitive config from available storages
    let redacted: Vec<_> = storages.into_iter().map(|s| s.redacted()).collect();
    Ok(HttpResponse::Ok().json(redacted))
}

// ─── Project member (user assignment) endpoints ──────────────────────────────

fn default_member_role() -> String {
    "member".to_string()
}

fn validate_member_role(role: &str) -> Result<(), AppError> {
    if role == "member" || role == "writer" {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "Invalid role '{}': must be 'member' or 'writer'",
            role
        )))
    }
}

#[derive(Debug, serde::Deserialize)]
struct AddMemberRequest {
    user_id: Uuid,
    #[serde(default = "default_member_role")]
    role: String,
}

#[derive(Debug, serde::Deserialize)]
struct UpdateMemberRoleRequest {
    role: String,
}

async fn list_project_members(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    user.require_admin()?;

    let project_id = path.into_inner();
    // Verify project exists
    Project::find_by_id(pool.get_ref(), project_id).await?;

    let members = UserProject::list_for_project(pool.get_ref(), project_id).await?;
    Ok(HttpResponse::Ok().json(members))
}

async fn add_project_member(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    user: AuthenticatedUser,
    body: web::Json<AddMemberRequest>,
) -> Result<HttpResponse, AppError> {
    user.require_admin()?;

    let project_id = path.into_inner();
    // Verify project and user exist
    Project::find_by_id(pool.get_ref(), project_id).await?;
    User::find_by_id(pool.get_ref(), body.user_id).await?;

    validate_member_role(&body.role)?;
    let assignment = UserProject::create(pool.get_ref(), body.user_id, project_id, &body.role).await?;
    Ok(HttpResponse::Created().json(assignment))
}

async fn update_project_member(
    pool: web::Data<PgPool>,
    path: web::Path<(Uuid, Uuid)>,
    user: AuthenticatedUser,
    body: web::Json<UpdateMemberRoleRequest>,
) -> Result<HttpResponse, AppError> {
    user.require_admin()?;

    let (project_id, member_user_id) = path.into_inner();
    validate_member_role(&body.role)?;
    let assignment = UserProject::update_role(pool.get_ref(), member_user_id, project_id, &body.role).await?;
    Ok(HttpResponse::Ok().json(assignment))
}

async fn remove_project_member(
    pool: web::Data<PgPool>,
    path: web::Path<(Uuid, Uuid)>,
    user: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    user.require_admin()?;

    let (project_id, member_user_id) = path.into_inner();
    UserProject::delete(pool.get_ref(), member_user_id, project_id).await?;
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
    )
    .service(
        web::resource("/projects/{id}/storages")
            .route(web::get().to(list_project_storages))
            .route(web::post().to(assign_storage)),
    )
    .service(
        web::resource("/projects/{id}/storages/{storage_id}")
            .route(web::put().to(update_project_storage))
            .route(web::delete().to(remove_storage_assignment)),
    )
    .service(
        web::resource("/projects/{id}/available-storages")
            .route(web::get().to(list_available_storages)),
    )
    .service(
        web::resource("/projects/{id}/members")
            .route(web::get().to(list_project_members))
            .route(web::post().to(add_project_member)),
    )
    .service(
        web::resource("/projects/{id}/members/{user_id}")
            .route(web::put().to(update_project_member))
            .route(web::delete().to(remove_project_member)),
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

    // ─── Auth enforcement tests ───────────────────────────────────────────────

    use crate::config::AuthConfig;
    use crate::services::auth_service::AuthService;

    fn test_auth_service() -> AuthService {
        AuthService::new(&AuthConfig {
            jwt_secret: "test-secret-for-project-endpoints".to_string(),
            access_token_ttl_secs: 900,
            refresh_token_ttl_secs: 604800,
            default_admin_username: "admin".to_string(),
            default_admin_password: "admin123".to_string(),
        })
    }

    #[actix_rt::test]
    async fn test_create_project_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/projects", actix_web::web::post().to(super::create_project)),
        )
        .await;

        let req = actix_web::test::TestRequest::post()
            .uri("/projects")
            .set_json(serde_json::json!({
                "name": "Test",
                "slug": "test"
            }))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_list_projects_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/projects", actix_web::web::get().to(super::list_projects)),
        )
        .await;

        let req = actix_web::test::TestRequest::get()
            .uri("/projects")
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_get_project_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/projects/{id}", actix_web::web::get().to(super::get_project)),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/projects/{}", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_update_project_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/projects/{id}", actix_web::web::put().to(super::update_project)),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::put()
            .uri(&format!("/projects/{}", id))
            .set_json(serde_json::json!({"name": "Updated"}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    // ─── Project member endpoint tests ──────────────────────────────────────

    #[actix_rt::test]
    async fn test_list_project_members_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{id}/members",
                    actix_web::web::get().to(super::list_project_members),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/projects/{}/members", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_list_project_members_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = uuid::Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{id}/members",
                    actix_web::web::get().to(super::list_project_members),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::get()
            .uri(&format!("/projects/{}/members", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_add_project_member_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{id}/members",
                    actix_web::web::post().to(super::add_project_member),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/members", id))
            .set_json(serde_json::json!({"user_id": uuid::Uuid::new_v4()}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_add_project_member_requires_admin() {
        let auth_service = test_auth_service();
        let user_id = uuid::Uuid::new_v4();
        let token = auth_service.generate_access_token(user_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{id}/members",
                    actix_web::web::post().to(super::add_project_member),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::post()
            .uri(&format!("/projects/{}/members", id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(serde_json::json!({"user_id": uuid::Uuid::new_v4()}))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_rt::test]
    async fn test_remove_project_member_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{id}/members/{user_id}",
                    actix_web::web::delete().to(super::remove_project_member),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let user_id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/projects/{}/members/{}", id, user_id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    #[actix_rt::test]
    async fn test_remove_project_member_requires_admin() {
        let auth_service = test_auth_service();
        let caller_id = uuid::Uuid::new_v4();
        let token = auth_service.generate_access_token(caller_id, "user").unwrap();

        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route(
                    "/projects/{id}/members/{user_id}",
                    actix_web::web::delete().to(super::remove_project_member),
                ),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let user_id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/projects/{}/members/{}", id, user_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[test]
    fn test_add_member_request_deserialization() {
        let json = serde_json::json!({"user_id": "550e8400-e29b-41d4-a716-446655440000"});
        let input: super::AddMemberRequest = serde_json::from_value(json).unwrap();
        assert_eq!(
            input.user_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_add_member_request_missing_user_id() {
        let json = serde_json::json!({});
        let result: Result<super::AddMemberRequest, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[actix_rt::test]
    async fn test_delete_project_requires_auth() {
        let auth_service = test_auth_service();
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(auth_service))
                .route("/projects/{id}", actix_web::web::delete().to(super::delete_project)),
        )
        .await;

        let id = uuid::Uuid::new_v4();
        let req = actix_web::test::TestRequest::delete()
            .uri(&format!("/projects/{}", id))
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }
}
