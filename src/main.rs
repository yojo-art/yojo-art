use std::{io::Write, net::SocketAddr, sync::Arc};

use axum::{
	http::StatusCode,
	response::{IntoResponse, Response},
};
use diesel_async::AsyncPgConnection;
use redis::aio::MultiplexedConnection;
use s3::Bucket;
use serde::{Deserialize, Serialize};
use service::{
	announcement::AnnouncementService, drive::DriveService, emoji::EmojiService,
	event::EventService, fanout_timeline::FanoutTimelineService, file_meta::FileMetaService,
	id_service::IdService, instance::InstanceService, meta::MetaService, note::NoteService,
	role::RoleService, token_service::TokenService, user::UserService,
};

use crate::service::timeline::TimelineService;
mod api;
mod browsersafe;
mod models;
mod service;

#[derive(Clone)]
pub struct ServerError {
	status: StatusCode,
	text: String,
}
impl IntoResponse for ServerError {
	fn into_response(self) -> axum::response::Response {
		(self.status, self.text).into_response()
	}
}
impl ServerError {
	pub fn new(status: StatusCode, text: String) -> Self {
		sentry::capture_message(&text, sentry::Level::Error);
		Self { status, text }
	}
	pub fn new_error<E>(status: StatusCode, e: E) -> Self
	where
		E: std::error::Error + Send + Sync + 'static,
	{
		let text = format!("{} {:?}",std::any::type_name::<E>(), e);
		sentry::integrations::anyhow::capture_anyhow(&anyhow::Error::new(e));
		Self { status, text }
	}
}
macro_rules! impl_convert_error_to_servererror {
	( $($t:ty),* ) => {
	$( impl From<$t> for ServerError
	{
		fn from(e:$t) -> Self
		{
			Self::new_error(StatusCode::INTERNAL_SERVER_ERROR,e)
		}
	}) *
	}
}
impl From<&str> for ServerError {
	fn from(s: &str) -> Self {
		Self {
			status: StatusCode::INTERNAL_SERVER_ERROR,
			text: format!("{:?}", s),
		}
	}
}
impl_convert_error_to_servererror! {
	diesel::result::Error,serde_json::Error,diesel_async::pooled_connection::bb8::RunError,redis::RedisError
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigFile {
	bind_addr: String,
	public_base_url: String,
	prefix: String,
	thumbnail_filter: FilterType,
	thumbnail_quality: f32,
	ffmpeg: Option<String>,
	ffmpeg_base_url: Option<String>,
	session_ttl: u64,
	part_max_size: u64,
	backend: String,
	full_upload_limit: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MisskeyConfig {
	id: String,
	db: DBConfig,
	url: String,
	#[serde(rename = "proxyRemoteFiles")]
	proxy_remote_files: Option<bool>,
	#[serde(rename = "mediaProxy")]
	media_proxy: Option<String>,
	#[serde(rename = "remoteProxy")]
	remote_proxy: Option<String>,
	#[serde(rename = "apFileBaseUrl")]
	ap_file_base_url: Option<String>,
	redis: RedisConfig,
	#[serde(rename = "redisForPubsub")]
	redis_for_pubsub: Option<RedisConfig>,
	#[serde(rename = "redisForTimelines")]
	redis_for_timelines: Option<RedisConfig>,
	#[serde(rename = "sentryForBackend")]
	sentry_for_backend: Option<SentryConfig>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedMisskeyConfig {
	id: String,
	url: String,
	proxy_remote_files: bool,
	media_proxy: Option<String>,
	remote_proxy: Option<String>,
	ap_file_base_url: Option<String>,
	host: String,
}
impl From<MisskeyConfig> for ParsedMisskeyConfig {
	fn from(f: MisskeyConfig) -> Self {
		let url = reqwest::Url::parse(f.url.as_str()).expect("url parse");
		let url_string = url.to_string();
		let host = url.host().expect("bad server url config").to_string();
		Self {
			id: f.id,
			url: url_string,
			proxy_remote_files: f.proxy_remote_files.unwrap_or(false),
			media_proxy: f.media_proxy,
			remote_proxy: f.remote_proxy,
			ap_file_base_url: f.ap_file_base_url,
			host,
		}
	}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SentryConfig {
	options: SentryOptions,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SentryOptions {
	dsn: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedisConfig {
	host: String,
	port: u16,
}
impl RedisConfig {
	fn to_url(&self) -> String {
		let host = self.host.as_str();
		let port = self.port;
		format!("redis://{host}:{port}")
	}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DBConfig {
	host: String,
	port: u16,
	db: String,
	user: String,
	pass: String,
}
impl DBConfig {
	fn to_url(&self) -> String {
		let user = self.user.as_str();
		let pass = self.pass.as_str();
		let host = self.host.as_str();
		let port = self.port;
		let db = self.db.as_str();
		format!("postgres://{user}:{pass}@{host}:{port}/{db}")
	}
}
#[derive(Clone, Debug)]
pub struct Context {
	config: Arc<ConfigFile>,
	pub misskey_config: Arc<ParsedMisskeyConfig>,
	pub redis: MultiplexedConnection,
	client: reqwest::Client,
	pub token_service: TokenService,
	pub role_service: RoleService,
	pub drive_service: DriveService,
	pub event_service: EventService,
	pub raw_db: DataBase,
	pub file_service: FileMetaService,
	pub user_service: UserService,
	pub meta_service: MetaService,
	pub timeline_service: TimelineService,
	pub fanout_timeline_service: FanoutTimelineService,
	pub note_service: NoteService,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum FilterType {
	Nearest,
	Triangle,
	CatmullRom,
	Gaussian,
	Lanczos3,
}
impl Into<image::imageops::FilterType> for FilterType {
	fn into(self) -> image::imageops::FilterType {
		match self {
			FilterType::Nearest => image::imageops::Nearest,
			FilterType::Triangle => image::imageops::Triangle,
			FilterType::CatmullRom => image::imageops::CatmullRom,
			FilterType::Gaussian => image::imageops::Gaussian,
			FilterType::Lanczos3 => image::imageops::Lanczos3,
		}
	}
}
impl Into<fast_image_resize::FilterType> for FilterType {
	fn into(self) -> fast_image_resize::FilterType {
		match self {
			FilterType::Nearest => fast_image_resize::FilterType::Box,
			FilterType::Triangle => fast_image_resize::FilterType::Bilinear,
			FilterType::CatmullRom => fast_image_resize::FilterType::CatmullRom,
			FilterType::Gaussian => fast_image_resize::FilterType::Mitchell,
			FilterType::Lanczos3 => fast_image_resize::FilterType::Lanczos3,
		}
	}
}
async fn shutdown_signal() {
	use futures::{future::FutureExt, pin_mut};
	use tokio::signal;
	let ctrl_c = async {
		signal::ctrl_c()
			.await
			.expect("failed to install Ctrl+C handler");
	}
	.fuse();

	#[cfg(unix)]
	let terminate = async {
		signal::unix::signal(signal::unix::SignalKind::terminate())
			.expect("failed to install signal handler")
			.recv()
			.await;
	}
	.fuse();
	#[cfg(not(unix))]
	let terminate = std::future::pending::<()>().fuse();
	pin_mut!(ctrl_c, terminate);
	futures::select! {
		_ = ctrl_c => {},
		_ = terminate => {},
	}
}
fn main() {
	let config_path = ".config/config.json";
	if !std::path::Path::new(&config_path).exists() {
		let default_config = ConfigFile {
			bind_addr: "0.0.0.0:12200".to_owned(),
			public_base_url: "https://files.example.com/files/".to_owned(),
			prefix: "prefix".to_owned(),
			thumbnail_filter: FilterType::Lanczos3,
			thumbnail_quality: 50f32,
			part_max_size: 20 * 1024 * 1024,
			ffmpeg: Some("ffmpeg".to_owned()),
			ffmpeg_base_url: Some("https://files.example.com/files/".to_owned()),
			full_upload_limit: 10 * 1024 * 1024,
			session_ttl: 300,
			backend: "http://localhost:3000".to_owned(),
		};
		let default_config = serde_json::to_string_pretty(&default_config).unwrap();
		std::fs::File::create(&config_path)
			.expect("create default config.json")
			.write_all(default_config.as_bytes())
			.unwrap();
	}
	let misskey_config: MisskeyConfig =
		serde_yaml::from_reader(std::fs::File::open(&".config/default.yml").unwrap()).unwrap();
	let _sentry_guard = if let Some(config) = misskey_config.sentry_for_backend.as_ref() {
		let sentry_guard = sentry::init((
			config.options.dsn.as_str(),
			sentry::ClientOptions {
				release: sentry::release_name!(),
				// Capture user IPs and potentially sensitive headers when using HTTP server integrations
				// see https://docs.sentry.io/platforms/rust/data-management/data-collected for more info
				send_default_pii: true,
				..Default::default()
			},
		));
		Some(sentry_guard)
	} else {
		None
	};
	let parsed_misskey_config: ParsedMisskeyConfig = misskey_config.clone().into();
	let misskey_config = Arc::new(misskey_config);
	let parsed_misskey_config = Arc::new(parsed_misskey_config);
	let file_service = FileMetaService::new();
	let config: ConfigFile =
		serde_json::from_reader(std::fs::File::open(&config_path).unwrap()).unwrap();
	let config = Arc::new(config);
	let redis = redis::Client::open(misskey_config.redis.to_url()).unwrap();
	let redis_for_pubsub = misskey_config
		.redis_for_pubsub
		.as_ref()
		.map(|redis_for_pubsub| redis::Client::open(redis_for_pubsub.to_url()).unwrap());
	let redis_for_timelines = misskey_config
		.redis_for_timelines
		.as_ref()
		.map(|redis_for_timelines| redis::Client::open(redis_for_timelines.to_url()).unwrap());
	let rt = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.unwrap();
	rt.block_on(async {
		let redis = redis
			.get_multiplexed_tokio_connection()
			.await
			.map_err(|e| println!("{:?}", e))
			.unwrap();
		let redis_for_pubsub = match redis_for_pubsub {
			Some(redis_for_pubsub) => redis_for_pubsub
				.get_multiplexed_tokio_connection()
				.await
				.ok(),
			None => None,
		};
		let redis_for_timelines = match redis_for_timelines {
			Some(redis_for_timelines) => redis_for_timelines
				.get_multiplexed_tokio_connection()
				.await
				.ok(),
			None => None,
		}
		.unwrap_or(redis.clone());
		let db = DataBase::open(&misskey_config.db.to_url()).await.unwrap();
		let id_service = IdService::new(&misskey_config);
		let token_service = TokenService::new(db.clone(), id_service.clone());
		let meta_service = MetaService::new(db.clone());
		let role_service = RoleService::new(db.clone(), meta_service.clone());
		let announcement_service = AnnouncementService::new(db.clone());
		let emoji_service = EmojiService::new(db.clone(), parsed_misskey_config.host.clone());
		let instance_service = InstanceService::new(db.clone(), redis.clone());
		let user_service = UserService::new(
			parsed_misskey_config.clone(),
			redis.clone(),
			db.clone(),
			id_service.clone(),
			role_service.clone(),
			announcement_service,
			emoji_service.clone(),
			instance_service.clone(),
			meta_service.clone(),
		);
		let event_service = EventService::new(
			redis_for_pubsub.clone().unwrap_or(redis.clone()),
			misskey_config.clone(),
		);
		let drive_service = DriveService::new(
			misskey_config.clone(),
			db.clone(),
			meta_service.clone(),
			role_service.clone(),
			id_service.clone(),
			user_service.clone(),
			event_service.clone(),
		);
		let note_service = NoteService::new(
			misskey_config.clone(),
			db.clone(),
			meta_service.clone(),
			role_service.clone(),
			drive_service.clone(),
			id_service.clone(),
			user_service.clone(),
			emoji_service.clone(),
			event_service.clone(),
		);
		let timeline_service = TimelineService::new(db.clone());
		let fanout_timeline_service = FanoutTimelineService::new(
			parsed_misskey_config.clone(),
			db.clone(),
			meta_service.clone(),
			redis_for_timelines,
			timeline_service.clone(),
		);
		let client = reqwest::Client::new();

		let arg_tup = Context {
			config,
			redis,
			client,
			token_service,
			role_service,
			drive_service,
			event_service,
			file_service,
			raw_db: db,
			user_service,
			meta_service,
			misskey_config: parsed_misskey_config,
			note_service,
			timeline_service,
			fanout_timeline_service,
		};
		let http_addr: SocketAddr = arg_tup.config.bind_addr.parse().unwrap();
		let app = api::endpoints::route(&arg_tup);
		let listener = tokio::net::TcpListener::bind(&http_addr).await.unwrap();
		println!("server loaded");
		axum::serve(
			listener,
			app.into_make_service_with_connect_info::<SocketAddr>(),
		)
		.with_graceful_shutdown(shutdown_signal())
		.await
		.unwrap();
	});
}
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadSession {
	user_id: String,
	s3_key: String,
	upload_id: Option<String>,
	content_type: String,
	part_etag: Vec<String>,
	part_number: Option<u32>,
	content_length: u64,
	md5_ctx_64: String,
	ext: Option<String>,
	comment: Option<String>,
	folder_id: Option<String>,
	is_sensitive: bool,
	force: bool,
	name: String,
	sensitive_threshold: f32,
	skip_sensitive_detection: bool,
}
pub(crate) fn md5_ontext_into_raw(ctx: md5::Context) -> String {
	let ptr = Box::leak(Box::new(ctx));
	let s = unsafe {
		std::slice::from_raw_parts(
			ptr as *const _ as *const u8,
			std::mem::size_of::<md5::Context>(),
		)
	};
	use base64::Engine;
	let s = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
	unsafe {
		let _ = Box::from_raw(ptr);
	}
	s
}
pub(crate) fn md5_ontext_from_raw(s: &String) -> md5::Context {
	use base64::Engine;
	let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
		.decode(s)
		.unwrap();
	let s = unsafe { Box::from_raw(raw.leak() as *mut _ as *mut md5::Context) };
	*s
}
impl Context {
	pub async fn upload_session(
		&mut self,
		authorization: Option<&axum::http::HeaderValue>,
		del: bool,
	) -> Result<(UploadSession, String), Response> {
		let session = match authorization.map(|v| {
			v.to_str().map(|s| {
				if s.starts_with("Bearer ") {
					Some(&s["Bearer ".len()..])
				} else {
					None
				}
			})
		}) {
			Some(Ok(Some(session_id))) => {
				let sid = {
					use sha2::{Digest, Sha256};
					let mut hasher = Sha256::new();
					hasher.update(session_id.as_bytes());
					let hash = hasher.finalize();
					use base64::Engine;
					base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
				};
				use redis::AsyncCommands;
				let res = if del {
					self.redis
						.get_del::<&String, String>(&format!("multipartUpload:{}", sid))
						.await
						.map(|v| serde_json::from_str::<UploadSession>(&v))
				} else {
					self.redis
						.get::<&String, String>(&format!("multipartUpload:{}", sid))
						.await
						.map(|v| serde_json::from_str::<UploadSession>(&v))
				};
				match res {
					Ok(Ok(s)) => Ok((s, sid)),
					Ok(Err(_)) => return Err((StatusCode::INTERNAL_SERVER_ERROR).into_response()),
					_ => return Err((StatusCode::FORBIDDEN).into_response()),
				}
			}
			e => {
				eprintln!("{}:{} {:?}", file!(), line!(), e);
				return Err((StatusCode::BAD_REQUEST).into_response());
			}
		};
		session
	}
}
#[derive(Clone, Debug)]
pub struct DataBase(diesel_async::pooled_connection::bb8::Pool<AsyncPgConnection>);
pub type DBConnection<'a> =
	diesel_async::pooled_connection::bb8::PooledConnection<'a, AsyncPgConnection>;

impl DataBase {
	pub async fn open(database_url: &str) -> Result<Self, String> {
		let config = diesel_async::pooled_connection::AsyncDieselConnectionManager::<
			AsyncPgConnection,
		>::new(database_url);
		let pool = match diesel_async::pooled_connection::bb8::Pool::builder()
			.build(config)
			.await
		{
			Ok(p) => p,
			Err(e) => return Err(e.to_string()),
		};
		Ok(Self(pool))
	}
	pub async fn get_writeable(&self) -> Option<DBConnection> {
		match self.0.get().await {
			Ok(c) => Some(c),
			Err(e) => {
				eprintln!("DB Error {:?}", e);
				None
			}
		}
	}
	pub async fn get_read_only(
		&self,
	) -> Result<DBConnection, diesel_async::pooled_connection::bb8::RunError> {
		self.0.get().await
	}
}
