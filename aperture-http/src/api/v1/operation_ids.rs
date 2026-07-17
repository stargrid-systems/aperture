//! Globally unique OpenAPI operation IDs.
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

/// `GET /api/v1/schedules`
pub const LIST_SCHEDULES: &str = "listSchedules";

/// `POST /api/v1/schedules`
pub const CREATE_SCHEDULE: &str = "createSchedule";

/// `GET /api/v1/schedules/{id}`
pub const GET_SCHEDULE: &str = "getSchedule";

/// `PATCH /api/v1/schedules/{id}`
pub const UPDATE_SCHEDULE: &str = "updateSchedule";

/// `DELETE /api/v1/schedules/{id}`
pub const DELETE_SCHEDULE: &str = "deleteSchedule";

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
