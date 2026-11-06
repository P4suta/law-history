use crate::{
    Store,
    amendment::{self, ParsedAmendment, Scenario, Simulation},
    model::*,
    references::{Impact, Reference, ReferenceReport},
    service::{self, BlameEntry, ParliamentaryContext, Status},
};
use axum::{
    Json, Router,
    extract::{Path, Query as HttpQuery, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};
use utoipa::{OpenApi, ToSchema};

const MAXIMUM_REQUEST_BYTES: usize = 2_097_152;
const MAXIMUM_ERROR_BYTES: usize = 65_536;

#[derive(Clone)]
pub struct ApiState {
    pub root: PathBuf,
    pub today: Option<Day>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    pub error: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct ApiError(HistoryError);
impl From<HistoryError> for ApiError {
    fn from(error: HistoryError) -> Self {
        Self(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            HistoryError::Invalid(_) | HistoryError::Json(_) => StatusCode::BAD_REQUEST,
            HistoryError::NotFound(_) => StatusCode::NOT_FOUND,
            HistoryError::Ambiguous(_) => StatusCode::CONFLICT,
            HistoryError::Storage(_) | HistoryError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let diagnostics = if let HistoryError::Ambiguous(items) = &self.0 {
            items.clone()
        } else {
            Vec::new()
        };
        let error = if status == StatusCode::INTERNAL_SERVER_ERROR {
            "Storage operation failed".into()
        } else {
            self.0.to_string()
        };
        (status, Json(ErrorBody { error, diagnostics })).into_response()
    }
}

async fn read<T, F>(state: Arc<ApiState>, operation: F) -> std::result::Result<Json<T>, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Store, Day) -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let today = match state.today {
            Some(day) => day,
            None => japan_day(Utc::now())?,
        };
        operation(&Store::open(&state.root)?, today)
    })
    .await
    .map_err(|_| ApiError(HistoryError::Invalid("Request worker failed".into())))?
    .map(Json)
    .map_err(ApiError)
}

#[derive(Debug, Default, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
    pub at: Option<Day>,
    pub known_at: Option<DateTime<Utc>>,
}

#[utoipa::path(get, path = "/api/search", params(("q" = Option<String>, Query), ("at" = Option<Day>, Query), ("known_at" = Option<String>, Query)), responses((status = 200, body = SearchResponse), (status = 409, body = ErrorBody)))]
async fn search(
    State(state): State<Arc<ApiState>>,
    HttpQuery(params): HttpQuery<SearchParams>,
) -> std::result::Result<Json<SearchResponse>, ApiError> {
    read(state, move |store, today| {
        service::search(
            store,
            &params.q,
            &Query {
                at: params.at,
                known_at: params.known_at,
                version: None,
            },
            today,
        )
    })
    .await
}

#[utoipa::path(get, path = "/api/laws/{law_id}", params(("law_id" = String, Path), ("at" = Option<Day>, Query), ("known_at" = Option<String>, Query), ("version" = Option<String>, Query)), responses((status = 200, body = Selection), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
async fn show(
    State(state): State<Arc<ApiState>>,
    Path(law): Path<String>,
    HttpQuery(query): HttpQuery<Query>,
) -> std::result::Result<Json<Selection>, ApiError> {
    read(state, move |store, today| {
        service::select(store, &law, &query, today)
    })
    .await
}

#[utoipa::path(get, path = "/api/laws/{law_id}/log", params(("law_id" = String, Path), ("known_at" = Option<String>, Query)), responses((status = 200, body = [StoredRevision])))]
async fn log(
    State(state): State<Arc<ApiState>>,
    Path(law): Path<String>,
    HttpQuery(query): HttpQuery<Query>,
) -> std::result::Result<Json<Vec<StoredRevision>>, ApiError> {
    read(state, move |store, _| store.history(&law, query.known_at)).await
}

#[derive(Debug, Deserialize)]
struct DiffParams {
    from: String,
    to: String,
    at: Option<Day>,
    known_at: Option<DateTime<Utc>>,
}

#[utoipa::path(get, path = "/api/diff", params(("from" = String, Query), ("to" = String, Query)), responses((status = 200, body = Diff), (status = 400, body = ErrorBody)))]
async fn diff(
    State(state): State<Arc<ApiState>>,
    HttpQuery(params): HttpQuery<DiffParams>,
) -> std::result::Result<Json<Diff>, ApiError> {
    read(state, move |store, _| {
        service::diff(store, &params.from, &params.to)
    })
    .await
}

#[utoipa::path(get, path = "/api/impact", params(("from" = String, Query), ("to" = String, Query), ("at" = Option<Day>, Query), ("known_at" = Option<String>, Query)), responses((status = 200, body = Impact)))]
async fn impact(
    State(state): State<Arc<ApiState>>,
    HttpQuery(params): HttpQuery<DiffParams>,
) -> std::result::Result<Json<Impact>, ApiError> {
    read(state, move |store, today| {
        service::impact(
            store,
            &params.from,
            &params.to,
            &Query {
                at: params.at,
                known_at: params.known_at,
                version: None,
            },
            today,
        )
    })
    .await
}

#[utoipa::path(get, path = "/api/laws/{law_id}/refs", params(("law_id" = String, Path), ("at" = Option<Day>, Query), ("known_at" = Option<String>, Query), ("version" = Option<String>, Query)), responses((status = 200, body = ReferenceReport)))]
async fn refs(
    State(state): State<Arc<ApiState>>,
    Path(law): Path<String>,
    HttpQuery(query): HttpQuery<Query>,
) -> std::result::Result<Json<ReferenceReport>, ApiError> {
    read(state, move |store, today| {
        service::refs(store, &law, &query, today)
    })
    .await
}

#[derive(Debug, Deserialize)]
struct BlameParams {
    path: String,
    at: Option<Day>,
    known_at: Option<DateTime<Utc>>,
    version: Option<String>,
}

#[utoipa::path(get, path = "/api/laws/{law_id}/blame", params(("law_id" = String, Path), ("path" = String, Query), ("at" = Option<Day>, Query), ("known_at" = Option<String>, Query), ("version" = Option<String>, Query)), responses((status = 200, body = BlameEntry)))]
async fn blame(
    State(state): State<Arc<ApiState>>,
    Path(law): Path<String>,
    HttpQuery(params): HttpQuery<BlameParams>,
) -> std::result::Result<Json<BlameEntry>, ApiError> {
    read(state, move |store, today| {
        service::blame(
            store,
            &law,
            &params.path,
            &Query {
                at: params.at,
                known_at: params.known_at,
                version: params.version,
            },
            today,
        )
    })
    .await
}

#[utoipa::path(post, path = "/api/simulate", request_body = Scenario, responses((status = 200, body = Simulation), (status = 400, body = ErrorBody)))]
async fn simulate(
    State(state): State<Arc<ApiState>>,
    Json(scenario): Json<Scenario>,
) -> std::result::Result<Json<Simulation>, ApiError> {
    read(state, move |store, _| service::simulate(store, &scenario)).await
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SavedScenario {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AmendmentInput {
    pub law_id: String,
    pub base_version: String,
    pub text: String,
    pub source_url: String,
}

#[utoipa::path(post, path = "/api/amendments/parse", request_body = AmendmentInput, responses((status = 200, body = ParsedAmendment), (status = 400, body = ErrorBody)))]
async fn parse_amendment(
    State(state): State<Arc<ApiState>>,
    Json(input): Json<AmendmentInput>,
) -> std::result::Result<Json<ParsedAmendment>, ApiError> {
    read(state, move |store, _| {
        let base = store.version(&input.base_version)?;
        if base.law_id != input.law_id {
            return Err(HistoryError::Invalid(
                "The base revision belongs to a different law".into(),
            ));
        }
        let evidence = Provenance {
            url: input.source_url,
            raw_sha256: store.preserve(input.text.as_bytes())?,
            media_type: "text/plain or application/xml".into(),
            attribution: "Imported amendment source".into(),
            terms_url: base.source.terms_url,
            confidence: Confidence::Inferred,
            locator: "source document".into(),
        };
        Ok(amendment::parse_document(
            &input.text,
            &input.law_id,
            &base.tree,
            &evidence,
        ))
    })
    .await
}

#[utoipa::path(post, path = "/api/scenarios", request_body = Scenario, responses((status = 200, body = SavedScenario), (status = 409, body = ErrorBody)))]
async fn save(
    State(state): State<Arc<ApiState>>,
    Json(scenario): Json<Scenario>,
) -> std::result::Result<Json<SavedScenario>, ApiError> {
    read(state, move |store, _| {
        Ok(SavedScenario {
            id: service::save_scenario(store, &scenario, Utc::now())?,
        })
    })
    .await
}

#[utoipa::path(get, path = "/api/scenarios/{id}", params(("id" = String, Path)), responses((status = 200, body = Scenario), (status = 404, body = ErrorBody)))]
async fn scenario(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Scenario>, ApiError> {
    read(state, move |store, _| {
        store
            .latest_fact(&format!("scenario:{id}"), None)?
            .ok_or_else(|| HistoryError::NotFound("Scenario not found".into()))
    })
    .await
}

#[utoipa::path(get, path = "/api/status", responses((status = 200, body = Status)))]
async fn status(State(state): State<Arc<ApiState>>) -> std::result::Result<Json<Status>, ApiError> {
    read(state, |store, _| service::status(store, None)).await
}

#[utoipa::path(get, path = "/api/laws/{law_id}/parliament", params(("law_id" = String, Path), ("known_at" = Option<String>, Query)), responses((status = 200, body = ParliamentaryContext)))]
async fn parliament(
    State(state): State<Arc<ApiState>>,
    Path(law): Path<String>,
    HttpQuery(query): HttpQuery<Query>,
) -> std::result::Result<Json<ParliamentaryContext>, ApiError> {
    read(state, move |store, _| {
        service::parliament(store, &law, query.known_at)
    })
    .await
}

#[derive(OpenApi)]
#[openapi(
    paths(
        search,
        show,
        log,
        diff,
        impact,
        refs,
        blame,
        simulate,
        save,
        scenario,
        status,
        parliament,
        parse_amendment
    ),
    components(schemas(
        ErrorBody,
        SavedScenario,
        Selection,
        Query,
        StoredRevision,
        Revision,
        Element,
        Content,
        LegalDate,
        Day,
        Provenance,
        Coverage,
        Diagnostic,
        DiagnosticCode,
        Confidence,
        RevisionKind,
        SearchHit,
        SearchResponse,
        Termination,
        TerminationStatus,
        Diff,
        Change,
        ChangeKind,
        LocatedElement,
        Scenario,
        Simulation,
        crate::amendment::AmendmentStep,
        crate::amendment::Operation,
        crate::amendment::SourcedOperation,
        crate::amendment::Adjustment,
        Reference,
        ReferenceReport,
        crate::references::ReferenceKind,
        crate::references::Position,
        crate::references::ReviewPath,
        Impact,
        BlameEntry,
        Status,
        crate::ingest::Catalog,
        ParliamentaryContext,
        crate::parliament::Bill,
        crate::parliament::BillId,
        crate::parliament::BillKind,
        crate::parliament::House,
        crate::parliament::SpeechLink,
        crate::parliament::Speech,
        AmendmentInput,
        ParsedAmendment
    ))
)]
struct ApiDoc;

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

pub type RouterType = Router;

pub async fn serve(listener: tokio::net::TcpListener, router: Router) -> anyhow::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("The shutdown signal could not be registered: {error}");
            }
        })
        .await?;
    Ok(())
}

pub fn router(state: ApiState, web: Option<PathBuf>) -> Router {
    let mut router = Router::new()
        .route("/api/search", get(search))
        .route("/api/laws/{law_id}", get(show))
        .route("/api/laws/{law_id}/log", get(log))
        .route("/api/diff", get(diff))
        .route("/api/impact", get(impact))
        .route("/api/laws/{law_id}/refs", get(refs))
        .route("/api/laws/{law_id}/blame", get(blame))
        .route("/api/simulate", post(simulate))
        .route("/api/amendments/parse", post(parse_amendment))
        .route("/api/scenarios", post(save))
        .route("/api/scenarios/{id}", get(scenario))
        .route("/api/status", get(status))
        .route("/api/laws/{law_id}/parliament", get(parliament))
        .route("/api/openapi.json", get(|| async { Json(openapi()) }))
        .layer(axum::extract::DefaultBodyLimit::max(MAXIMUM_REQUEST_BYTES))
        .layer(axum::middleware::from_fn(normalize_errors))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .with_state(Arc::new(state));
    if let Some(web) = web {
        router = router.fallback_service(
            ServeDir::new(&web).not_found_service(ServeFile::new(web.join("index.html"))),
        );
    }
    router
}

async fn normalize_errors(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let response = next.run(request).await;
    if response.status().is_client_error()
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_none_or(|value| value != "application/json")
    {
        let status = response.status();
        let error = match axum::body::to_bytes(response.into_body(), MAXIMUM_ERROR_BYTES).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => format!(
                "Request rejected; the diagnostic exceeded the response limit or could not be read: {error}"
            ),
        };
        return (
            status,
            Json(ErrorBody {
                error,
                diagnostics: Vec::new(),
            }),
        )
            .into_response();
    }
    response
}
