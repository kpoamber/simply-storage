# Система авторизации (JWT + роли admin/user)

## Overview

Добавить систему аутентификации и авторизации на основе JWT токенов (access + refresh) с ролями на уровне системы (admin/user). Admin имеет полный доступ, user - только к своим проектам.

## Context

- Files involved: `src/api/mod.rs`, `src/api/auth.rs` (new), `src/db/models.rs`, `src/config.rs`, `src/error.rs`, `src/main.rs`, `src/lib.rs`, `migrations/005_users.sql` (new), `frontend/src/` (auth pages, context, api)
- Related patterns: AppError enum с ResponseError, AppConfig с serde defaults, API routes через web::scope, sqlx FromRow модели в db/models.rs
- Dependencies: `jsonwebtoken`, `argon2` (password hashing)

## Development Approach

- **Testing approach**: Regular (code first, then tests)
- Complete each task fully before moving to the next
- **CRITICAL: every task MUST include new/updated tests**
- **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: Миграция БД и модель User

**Files:**
- Create: `migrations/005_users.sql`
- Modify: `src/db/models.rs`

- [x] Создать миграцию `005_users.sql`:
  - Таблица `users`: id (UUID PK default gen_random_uuid()), username (VARCHAR UNIQUE NOT NULL), password_hash (VARCHAR NOT NULL), role (VARCHAR NOT NULL DEFAULT 'user' CHECK IN ('admin','user')), created_at, updated_at
  - Таблица `refresh_tokens`: id (UUID PK), user_id (UUID FK -> users ON DELETE CASCADE), token_hash (VARCHAR NOT NULL), expires_at (TIMESTAMP NOT NULL), created_at
  - Добавить колонку `owner_id UUID REFERENCES users(id)` в таблицу `projects`
- [x] Добавить модель User в `src/db/models.rs` с CRUD: create, find_by_id, find_by_username
- [x] Добавить модель RefreshToken в `src/db/models.rs`: create, find_by_hash, delete_by_user_id, delete_expired
- [x] Обновить модель Project: добавить поле owner_id (Option<Uuid>)
- [x] Написать unit тесты для сериализации/десериализации моделей User и RefreshToken
- [x] run project test suite - must pass before task 2

### Task 2: Конфигурация JWT и сервис авторизации

**Files:**
- Modify: `src/config.rs`
- Create: `src/services/auth_service.rs`
- Modify: `src/services/mod.rs`
- Modify: `Cargo.toml`

- [x] Добавить зависимости в Cargo.toml: `jsonwebtoken = "9"`, `argon2 = "0.5"`
- [x] Добавить AuthConfig в `src/config.rs`: jwt_secret (String), access_token_ttl_secs (u64, default 900), refresh_token_ttl_secs (u64, default 604800)
- [x] Создать `src/services/auth_service.rs` с AuthService:
  - `hash_password(password) -> String` - хеширование через argon2
  - `verify_password(password, hash) -> bool` - проверка пароля
  - `generate_access_token(user_id, role) -> String` - JWT access token
  - `generate_refresh_token() -> String` - случайный refresh token
  - `validate_access_token(token) -> Claims` - валидация и декодирование JWT
- [x] Claims struct: sub (user_id), role (String), exp (usize)
- [x] Написать тесты: hash/verify password, generate/validate token, expired token rejection
- [x] run project test suite - must pass before task 3

### Task 3: Auth middleware (extractor)

**Files:**
- Create: `src/api/auth.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/error.rs`

- [x] Добавить вариант `Forbidden(String)` в AppError с кодом 403
- [x] Создать `src/api/auth.rs` с Actix-Web extractor `AuthenticatedUser`:
  - Реализовать `FromRequest` для AuthenticatedUser
  - Извлекать JWT из заголовка `Authorization: Bearer <token>`
  - Декодировать и валидировать токен через AuthService
  - Структура AuthenticatedUser: user_id (Uuid), role (String)
- [x] Добавить helper-метод `AuthenticatedUser::require_admin() -> Result<(), AppError>` для проверки роли
- [x] Добавить helper `AuthenticatedUser::require_owner_or_admin(owner_id) -> Result<(), AppError>`
- [x] Написать тесты: извлечение из валидного токена, отклонение без токена, отклонение с истекшим токеном, require_admin проверки
- [x] run project test suite - must pass before task 4

### Task 4: API эндпоинты авторизации

**Files:**
- Create: `src/api/auth_routes.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`

- [x] Создать `src/api/auth_routes.rs` с эндпоинтами:
  - `POST /api/auth/register` - регистрация (username, password) -> user + tokens
  - `POST /api/auth/login` - вход (username, password) -> access_token + refresh_token
  - `POST /api/auth/refresh` - обновление access_token по refresh_token
  - `GET /api/auth/me` - информация о текущем пользователе (требует AuthenticatedUser)
  - `POST /api/auth/logout` - удаление refresh_token
- [x] Первый зарегистрированный пользователь автоматически получает роль admin
- [x] Добавить AuthService в app_data в main.rs
- [x] Зарегистрировать auth routes в configure_api_routes
- [x] Написать тесты для auth endpoints (register, login, refresh, me, logout)
- [x] run project test suite - must pass before task 5

### Task 5: Защита существующих API эндпоинтов

**Files:**
- Modify: `src/api/projects.rs`
- Modify: `src/api/files.rs`
- Modify: `src/api/storages.rs`
- Modify: `src/api/bulk.rs`
- Modify: `src/api/mod.rs`

- [x] Добавить AuthenticatedUser extractor во все обработчики в projects.rs:
  - Создание проекта: записывать owner_id = user_id
  - Просмотр/изменение/удаление проекта: проверять owner_or_admin
  - Список проектов: admin видит все, user только свои
- [x] Добавить AuthenticatedUser extractor в files.rs:
  - Загрузка/скачивание файлов: проверять доступ к проекту через owner_or_admin
- [x] Добавить AuthenticatedUser extractor в storages.rs и bulk.rs:
  - Управление хранилищами: только admin
- [x] Защитить system endpoints (stats, config-export, nodes): только admin
- [x] Обновить существующие тесты с mock-авторизацией
- [x] run project test suite - must pass before task 6

### Task 6: Frontend - аутентификация

**Files:**
- Create: `frontend/src/contexts/AuthContext.tsx`
- Create: `frontend/src/pages/Login.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/components/Layout.tsx`
- Modify: `frontend/src/components/Sidebar.tsx`

- [x] Добавить типы AuthUser, LoginRequest, LoginResponse, RegisterRequest в types.ts
- [x] Создать AuthContext с:
  - Хранение access_token в памяти, refresh_token в localStorage
  - Функции login, register, logout, refreshToken
  - Axios interceptor: добавлять Authorization header, автоматический refresh при 401
- [x] Создать страницу Login (login + register форма)
- [x] Обновить App.tsx: добавить AuthProvider, ProtectedRoute компонент, route /login
- [x] Обновить Layout/Sidebar: показывать username и кнопку logout, скрывать admin-only пункты меню для роли user
- [x] Написать тесты: Login page рендеринг, AuthContext login/logout, ProtectedRoute redirect
- [x] run project test suite - must pass before task 7

### Task 7: Проверка acceptance criteria

- [x] manual test: регистрация первого пользователя (должен стать admin)
- [x] manual test: login -> получение токенов -> доступ к API
- [x] manual test: создание проекта user-ом -> доступ только к своим проектам
- [x] manual test: admin видит все проекты и может управлять хранилищами
- [x] manual test: refresh token обновляет access token
- [x] manual test: frontend redirect на /login без авторизации
- [x] run full test suite: `cargo test` и `cd frontend && npm test`
- [x] run linter: `cargo clippy -- -D warnings` и `cd frontend && npm run lint`

### Task 8: Обновление документации

- [x] Обновить CLAUDE.md: добавить auth-related файлы в Project Structure
- [x] Добавить env vars для JWT конфигурации в раздел Configuration Env Vars
- [x] Переместить план в `docs/plans/completed/`
