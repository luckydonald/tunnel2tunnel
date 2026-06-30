Perfect! I've read all 11 files completely. Here's a comprehensive summary of the patterns and architecture:

## Architecture Overview

### 1. **Backend - Rust/Axum Stack** (tunnel2tunnel-core & tunnel2tunnel-web)

#### Database Models (Entity-based):
- **Entity** (`entity.rs`): Core domain entity with user ownership, type (server/client), timestamps with soft-delete
- **EntityPort** (`entity_port.rs`): Configured ports for an entity with local/proxy mapping, sort order, enable flag
- **EntityAccess** (`entity_access.rs`): Access control rules with multiple subject types (public_lite, all_mine, all_user_entities, entity)

#### Naming Patterns:
- Models use struct derives: `sqlx::FromRow`, `Serialize`, `Clone`, `Debug`
- Timestamp fields wrapped in `Timestamps` or `TimestampsSoftDelete` flattened structs
- DB methods are async and static: `create()`, `update()`, `delete()`, `list_for_*()`, `find_by_*()`, `soft_delete()`
- Return `Result<T, CoreError>` consistently

#### Web API Routes (entities.rs):
- Request/response DTOs separate from models (e.g., `CreateEntityBody`, `EntityResponse`)
- Handlers take `AuthUser` extractor + `State(AppState)` + path/query/body parameters
- Response status codes: `StatusCode::CREATED` (201), `StatusCode::NO_CONTENT` (204)
- Error handling via `WebError` enum
- Helper: `require_owner()` validates entity ownership before mutation

### 2. **Database Schema**

#### Key Tables:
- `entities`: id, user_id, type, name, description, ip_whitelist, valid_until, created_at, updated_at, deleted_at (soft delete)
- `entity_ports`: id, entity_id, enabled, local_port, proxy_port, name, description, sort_order, created_at, updated_at
- `entity_access`: id, owner_entity_id, subject_type, subject_entity_id, subject_user_id, hostname, created_at, updated_at
- `friendships`: id, from_user_id, to_user_id, status, visibility_grant
- `friendship_entity_grants`: id, friendship_id, entity_id
- `connection_logs`: id, entity_id, peer_ip, key_fingerprint, login_succeeded, failure_reason, ssh_flags, ports_requested, started_at, ended_at

#### Patterns:
- Automatic `created_at`/`updated_at` via PostgreSQL triggers (`set_timestamps()`)
- UUID v7 as primary keys
- Cascade deletes on foreign keys
- Check constraints for enum-like fields (e.g., subject_type IN (…))

### 3. **Frontend - Vue 3/TypeScript Stack**

#### API Layer (`api/entities.ts`):
- TypeScript interfaces for all response types mirror backend DTOs
- API wrapper function `apiFetch<T>()` handles auth, JSON serialization, error parsing
- API object `entitiesApi` with methods returning promises typed to response interfaces
- Params interfaces for create/update operations (with optional fields where appropriate)

#### Component Patterns:
- **SshCommandDisplay.vue**: Stateless display component showing SSH CLI command based on entity type and ports
- **EntityDetailPage.vue**: Full-featured detail view with lazy-loading sections (access rules, logs)
  - Local state for forms (ports, keys, access rules)
  - Confirmation dialogs for destructive actions
  - localStorage persistence for user preferences (key filename)
  - Computed properties for lookup maps (entityNameMap, friendNameMap)
  - Derived helpers (e.g., `friendUserId()`)

#### Frontend Conventions:
- Boolean state tracking: `loading`, `logsLoaded`, `logsLoading`, `showAddPort`, etc.
- Form data refs like `newPort`, `newAccess` with `blankAccess()` factory
- Error handling via alerts, displayed errors in templates
- Button states follow: "Action", "Acting...", "Cancel" pattern
- Accessibility: labels for inputs, disabled states on buttons during async ops

### 4. **HTTP Route Structure** (from lib.rs):

```
POST   /api/entities                                → create_entity
GET    /api/entities                                → list_entities
PUT    /api/entities/{id}                           → update_entity
DELETE /api/entities/{id}                           → delete_entity
POST   /api/entities/{entity_id}/keys               → add_key
DELETE /api/entities/{entity_id}/keys/{key_id}      → delete_key
GET    /api/entities/{entity_id}/ports              → list_ports
POST   /api/entities/{entity_id}/ports              → create_port
PUT    /api/entities/{entity_id}/ports/{port_id}    → update_port
DELETE /api/entities/{entity_id}/ports/{port_id}    → delete_port
GET    /api/entities/{entity_id}/access             → list_access
POST   /api/entities/{entity_id}/access             → create_access
DELETE /api/entities/{entity_id}/access/{rule_id}   → delete_access
GET    /api/entities/{id}/logs                      → list_connection_logs
```

### 5. **Key Design Patterns to Follow**

1. **Ownership validation**: Always check `user_id` matches before returning/mutating
2. **Timestamps**: Automatic via DB triggers, flatten into responses
3. **Soft deletes**: Filter with `deleted_at IS NULL` in selects
4. **UUIDs v7**: Use `Uuid::now_v7()` for new IDs
5. **Optionals in DB/API**: Use `Option<String>`, `Option<Uuid>` with NULL handling
6. **Optional fields in request bodies**: Mark as `Option<T>` in DTOs
7. **Async all the way**: All database operations are async
8. **Error propagation**: Use `?` operator with `map_err(CoreError::Sqlx)`
9. **Lazy loading in UI**: Load form data on-demand when section opens
10. **Confirmation dialogs**: Use `confirm()` for destructive operations

All files are now at your disposal for implementation. Would you like me to search for anything specific or help you implement a particular feature following these patterns?