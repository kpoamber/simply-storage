# CI/CD Pipeline: Hetzner Cloud (Terraform) + Windows Server (SSH Deploy)

## Overview

Полноценный CI/CD pipeline через GitHub Actions: CI (тесты, линтинг), сборка Docker-образа, автоматический деплой на Hetzner Cloud через Terraform и на Windows Server через SSH. Три профиля масштабируемости: small, medium, large. Автоматические бэкапы PostgreSQL/Citus с возможностью восстановления сервера из бэкапа базы.

## Context

- Files involved: `.github/workflows/`, `docker-compose.yml`, `docker/nginx.conf`, `Dockerfile`
- New directories: `terraform/`, `deploy/`
- Related patterns: existing `build-push.yml` workflow, multi-stage Dockerfile, docker-compose with `APP_REPLICAS`
- Dependencies: Terraform (hashicorp/hcloud provider), GitHub Environments/Secrets

## Development Approach

- **Testing approach**: Manual verification of workflows via GitHub Actions runs
- Infrastructure-as-Code files tested via `terraform validate` and `terraform plan`
- Each task is a logical unit that can be tested independently
- **CRITICAL: every task MUST include validation steps**
- **CRITICAL: all validation must pass before starting next task**

## Implementation Steps

### Task 1: CI Pipeline - Tests and Linting

**Files:**
- Create: `.github/workflows/ci.yml`

- [x] Create CI workflow triggered on push to any branch and PRs to main
- [x] Add job `backend-checks`: cargo clippy -- -D warnings, cargo test
- [x] Add job `frontend-checks`: npm ci, npm run lint, npm run build (в frontend/)
- [x] Add job `docker-build-test`: docker build без push (проверка что образ собирается)
- [x] Настроить кэширование: cargo registry/target через actions/cache, npm через actions/cache
- [x] Verify: push to non-main branch triggers CI but NOT build-push

### Task 2: Production Docker Compose и профили масштабируемости

**Files:**
- Create: `deploy/docker-compose.prod.yml` (базовый production compose, использует GHCR образ)
- Create: `deploy/docker-compose.small.yml` (override: 1 app replica, standalone Postgres без Citus workers)
- Create: `deploy/docker-compose.medium.yml` (override: 2 app replicas, Citus с 2 workers)
- Create: `deploy/docker-compose.large.yml` (override: 4 app replicas, Citus с 4 workers, увеличенные ресурсы)
- Create: `deploy/.env.example` (шаблон переменных окружения)
- Create: `deploy/docker/nginx-prod.conf` (production nginx с TLS через Certbot/Let's Encrypt)

- [ ] Создать `docker-compose.prod.yml`: image из GHCR вместо build, переменные из .env файла, именованные volumes для данных и БД
- [ ] Создать `docker-compose.small.yml`: 1 app replica, один postgres без citus workers, сниженные лимиты памяти
- [ ] Создать `docker-compose.medium.yml`: 2 app replicas, citus coordinator + 2 workers (как текущий docker-compose.yml)
- [ ] Создать `docker-compose.large.yml`: 4 app replicas, citus coordinator + 4 workers, увеличенные connection pools и буферы
- [ ] Создать `.env.example` с документированными переменными: IMAGE_TAG, POSTGRES_PASSWORD, APP_AUTH__JWT_SECRET, DOMAIN, BACKUP_RETENTION_DAYS, BACKUP_SCHEDULE и т.д.
- [ ] Создать production nginx.conf с TLS конфигурацией (certbot-совместимой)
- [ ] Verify: `docker compose -f deploy/docker-compose.prod.yml -f deploy/docker-compose.small.yml config` валиден

### Task 3: Terraform для Hetzner Cloud

**Files:**
- Create: `terraform/main.tf`
- Create: `terraform/variables.tf`
- Create: `terraform/outputs.tf`
- Create: `terraform/versions.tf`
- Create: `terraform/cloud-init.yml` (скрипт первоначальной настройки сервера)
- Create: `terraform/tfvars/small.tfvars`
- Create: `terraform/tfvars/medium.tfvars`
- Create: `terraform/tfvars/large.tfvars`

- [ ] Создать `versions.tf`: required_providers (hetznercloud/hcloud), terraform backend config (local state, с комментарием для remote backend)
- [ ] Создать `variables.tf`: hcloud_token, server_type, location, ssh_key_name, domain, deploy_profile (small/medium/large), app_env_vars (map), backup_volume_size
- [ ] Создать `main.tf`: hcloud_ssh_key, hcloud_server, hcloud_firewall (порты 22, 80, 443), hcloud_network + subnet (для multi-node), hcloud_volume для бэкапов
- [ ] Создать `cloud-init.yml`: установка Docker + Docker Compose, создание пользователя deploy, настройка SSH, монтирование volume для бэкапов, настройка cron для автоматических бэкапов
- [ ] Создать tfvars для профилей: small (cx22 - 2vCPU/4GB), medium (cx32 - 4vCPU/8GB), large (cx42 - 8vCPU/16GB)
- [ ] Создать `outputs.tf`: server_ip, server_status, ssh_connection_string, backup_volume_id
- [ ] Verify: `terraform validate` и `terraform plan` без ошибок (dry-run)

### Task 4: Database Backup и Restore

**Files:**
- Create: `deploy/scripts/backup.sh` (скрипт бэкапа PostgreSQL/Citus)
- Create: `deploy/scripts/restore.sh` (скрипт восстановления из бэкапа)
- Create: `deploy/scripts/backup-cron.sh` (обёртка для cron с логированием и ротацией)
- Create: `.github/workflows/backup.yml` (ручной и scheduled workflow для бэкапов)
- Create: `.github/workflows/restore.yml` (ручной workflow для восстановления)

- [ ] Создать `backup.sh`: pg_dump для small-профиля (standalone postgres), pg_dump coordinator + каждого worker для medium/large (Citus), сжатие через gzip, именование файлов с timestamp, копирование на backup volume
- [ ] Создать `backup-cron.sh`: вызов backup.sh с логированием в файл, ротация бэкапов по BACKUP_RETENTION_DAYS (удаление старых), отправка уведомления при ошибке (опционально через webhook)
- [ ] Создать `restore.sh`: принимает путь к бэкапу или дату, останавливает app-контейнеры, восстанавливает БД через psql (coordinator + workers для Citus), запускает миграции если нужно, перезапускает app-контейнеры, health check
- [ ] Создать GitHub workflow `backup.yml`: schedule (ежедневно, настраиваемый cron) + workflow_dispatch, SSH на сервер и запуск backup.sh, upload artifact с бэкапом (опционально)
- [ ] Создать GitHub workflow `restore.yml`: workflow_dispatch с input backup_date или backup_file, SSH на сервер, запуск restore.sh, health check после восстановления
- [ ] Добавить в docker-compose.prod.yml volume для бэкапов: `backups:/backups`
- [ ] Verify: backup.sh и restore.sh проходят shellcheck, workflows синтаксически валидны

### Task 5: Deploy Workflow - Hetzner Cloud

**Files:**
- Create: `.github/workflows/deploy-hetzner.yml`
- Create: `deploy/scripts/deploy.sh` (скрипт деплоя на сервере)

- [ ] Создать deploy.sh: pull GHCR image, подставить .env, запуск pre-deploy backup (вызов backup.sh), docker compose up с нужным профилем, health check, rollback при failure (восстановление из pre-deploy бэкапа)
- [ ] Создать workflow triggered on: workflow_dispatch (manual) с inputs: environment (staging/production), profile (small/medium/large)
- [ ] Добавить job `deploy`: SSH на Hetzner сервер, копирование deploy/ файлов, запуск deploy.sh
- [ ] Использовать GitHub Environments (staging, production) с required reviewers для production
- [ ] Secrets: HETZNER_SSH_KEY, HETZNER_HOST, DEPLOY_ENV (через environment secrets)
- [ ] Добавить post-deploy health check: curl /health endpoint
- [ ] Verify: workflow syntax валиден, все secrets задокументированы

### Task 6: Deploy Workflow - Windows Server

**Files:**
- Create: `.github/workflows/deploy-windows.yml`
- Create: `deploy/scripts/deploy-windows.sh` (скрипт деплоя через SSH на Windows с Docker)

- [ ] Создать deploy-windows.sh: SSH на Windows Server, pre-deploy backup, docker compose pull, docker compose up -d с профилем, health check, rollback при failure
- [ ] Создать workflow triggered on: workflow_dispatch с inputs: profile (small/medium/large)
- [ ] Добавить job `deploy`: SSH на Windows Server (через OpenSSH), передача docker-compose файлов, запуск deploy скрипта
- [ ] Secrets: WINDOWS_SSH_KEY, WINDOWS_HOST, WINDOWS_USER
- [ ] Health check после деплоя
- [ ] Verify: workflow syntax валиден

### Task 7: Обновить существующий build-push.yml

**Files:**
- Modify: `.github/workflows/build-push.yml`

- [ ] Добавить зависимость от CI workflow (needs: ci) - деплой только после прохождения тестов
- [ ] Добавить тегирование образа по semver тегам (type=semver для git tags v*)
- [ ] Добавить output image_tag для использования в deploy workflows
- [ ] Добавить optional auto-trigger deploy workflows после успешного push (через workflow_call)
- [ ] Verify: push в main вызывает CI -> Build -> (optional) Deploy chain

### Task 8: Verify acceptance criteria

- [ ] CI workflow запускается на PR и push, включает backend и frontend проверки
- [ ] Production compose файлы валидны для всех 3 профилей (small, medium, large)
- [ ] Terraform конфигурация проходит `terraform validate`
- [ ] Deploy workflows для Hetzner и Windows имеют health checks и rollback
- [ ] Backup workflow работает по расписанию и вручную
- [ ] Restore workflow позволяет восстановить БД из бэкапа с указанием даты
- [ ] Deploy скрипты делают pre-deploy backup и rollback при failure
- [ ] Все секреты задокументированы в .env.example и README
- [ ] `docker compose -f deploy/docker-compose.prod.yml -f deploy/docker-compose.{small,medium,large}.yml config` работает для каждого профиля

### Task 9: Update documentation

- [ ] Обновить README.md: секция Deployment с описанием CI/CD pipeline, профилей, настройки секретов, бэкапов и восстановления
- [ ] Обновить CLAUDE.md: добавить информацию о deploy/, terraform/ структуре, CI/CD workflows, и backup/restore скриптах
- [ ] Move this plan to `docs/plans/completed/`
