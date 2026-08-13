//! Globally unique `OpenAPI` operation IDs.
//!
//! Every operation in the API must have a unique operation id. These consts
//! are the single source of truth so that there is no risk of collisions
//! between handlers spread across multiple files. The generated TypeScript
//! client uses these as method names.

/// `GET /api/v1/version`
pub const GET_GATEWAY_VERSION: &str = "getGatewayVersion";

/// `GET /api/v1/artifacts`
pub const LIST_ARTIFACTS: &str = "listArtifacts";

/// `GET /api/v1/artifacts/{key}`
pub const GET_ARTIFACT: &str = "getArtifact";

/// `GET /api/v1/artifacts/{key}/versions`
pub const LIST_ARTIFACT_VERSIONS: &str = "listArtifactVersions";

/// `GET /api/v1/artifacts/{key}/versions/{digest}`
pub const GET_ARTIFACT_VERSION: &str = "getArtifactVersion";

/// `DELETE /api/v1/artifacts/{key}/versions/{digest}`
pub const DELETE_ARTIFACT_VERSION: &str = "deleteArtifactVersion";

/// `PUT /api/v1/artifacts/{key}`
pub const UPLOAD_ARTIFACT: &str = "uploadArtifact";

/// `GET /api/v1/artifacts/{key}/versions/{digest}/blob`
pub const DOWNLOAD_ARTIFACT_BLOB: &str = "downloadArtifactBlob";

/// `GET /api/v1/tasks`
pub const LIST_TASKS: &str = "listTasks";

/// `POST /api/v1/tasks`
pub const CREATE_TASK: &str = "createTask";

/// `GET /api/v1/tasks/{id}`
pub const GET_TASK: &str = "getTask";

/// `POST /api/v1/tasks/{id}/cancel`
pub const CANCEL_TASK: &str = "cancelTask";

/// `GET /api/v1/task-definitions`
pub const LIST_TASK_DEFINITIONS: &str = "listTaskDefinitions";

/// `GET /api/v1/task-schedules`
pub const LIST_TASK_SCHEDULES: &str = "listTaskSchedules";

/// `POST /api/v1/task-schedules`
pub const CREATE_TASK_SCHEDULE: &str = "createTaskSchedule";

/// `GET /api/v1/task-schedules/{id}`
pub const GET_TASK_SCHEDULE: &str = "getTaskSchedule";

/// `PATCH /api/v1/task-schedules/{id}`
pub const UPDATE_TASK_SCHEDULE: &str = "updateTaskSchedule";

/// `DELETE /api/v1/task-schedules/{id}`
pub const DELETE_TASK_SCHEDULE: &str = "deleteTaskSchedule";

/// `GET /api/v1/logs`
pub const LIST_LOGS: &str = "listLogs";

/// `GET /api/v1/logs/targets`
pub const LIST_LOG_TARGETS: &str = "listLogTargets";

/// `GET /api/v1/logs/spans`
pub const LIST_SPANS: &str = "listSpans";

/// `GET /api/v1/logs/spans/{id}`
pub const GET_SPAN: &str = "getSpan";

/// `GET /api/v1/logs/boots`
pub const LIST_LOG_BOOTS: &str = "listLogBoots";

/// `POST /api/v1/auth/login`
pub const LOGIN: &str = "login";

/// `POST /api/v1/auth/logout`
pub const LOGOUT: &str = "logout";

/// `GET /api/v1/auth/me`
pub const GET_CURRENT_USER: &str = "getCurrentUser";

/// `POST /api/v1/auth/change-password`
pub const CHANGE_PASSWORD: &str = "changePassword";

/// `GET /api/v1/auth/setup-status`
pub const SETUP_STATUS: &str = "getSetupStatus";

/// `POST /api/v1/auth/setup`
pub const SETUP: &str = "setup";

/// `GET /api/v1/users`
pub const LIST_USERS: &str = "listUsers";

/// `POST /api/v1/users`
pub const CREATE_USER: &str = "createUser";

/// `GET /api/v1/users/{id}`
pub const GET_USER: &str = "getUser";

/// `DELETE /api/v1/users/{id}`
pub const DELETE_USER: &str = "deleteUser";

/// `GET /api/v1/users/{id}/avatar`
pub const GET_USER_AVATAR: &str = "getUserAvatar";

/// `GET /api/v1/api-keys`
pub const LIST_API_KEYS: &str = "listApiKeys";

/// `POST /api/v1/api-keys`
pub const CREATE_API_KEY: &str = "createApiKey";

/// `DELETE /api/v1/api-keys/{id}`
pub const DELETE_API_KEY: &str = "deleteApiKey";

/// `GET /api/v1/settings`
pub const LIST_SETTINGS: &str = "listSettings";

/// `GET /api/v1/settings/{scope}`
pub const GET_SETTING: &str = "getSetting";

/// `PUT /api/v1/settings/{scope}`
pub const UPDATE_SETTING: &str = "updateSetting";
