use axum::{http::StatusCode, response::IntoResponse};
use futures::TryStreamExt;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

use crate::{models::user::MiUser, Context, UploadSession};

#[derive(Debug, Serialize,Deserialize)]
pub struct RequestBody{
	i: String,//トークン必須
}
pub async fn post(
	mut ctx:Context,
	request: axum::extract::Request,
)->axum::response::Response{
	let authorization=request.headers().get("Authorization");
	let (session,_hashed_sid)=match ctx.upload_session(authorization,true).await{
		Ok(v)=>v,
		Err(e)=>return e,
	};
	let stream=request.into_body().into_data_stream();
	let body_with_io_error = stream.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));
	let mut body_reader = StreamReader::new(body_with_io_error);
	let mut buf=vec![];
	if let Err(_e)=body_reader.read_to_end(&mut buf).await{
		return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
	}
	let q=match serde_json::from_slice::<RequestBody>(&buf){
		Ok(v)=>v,
		Err(e)=>{
			eprintln!("{}:{} {:?}",file!(),line!(),e);
			return (StatusCode::BAD_REQUEST).into_response()
		}
	};
	let mut parts=vec![];
	let mut part_number=1;
	for etag in session.part_etag.iter(){
		async fn err_handle(ctx: &Context,session: &UploadSession)->axum::response::Response{
			if let Some(upload_id)=session.upload_id.as_ref(){
				let _=ctx.bucket.abort_upload(&session.s3_key,upload_id).await;
			}
			(StatusCode::INTERNAL_SERVER_ERROR).into_response()
		}
		let mut error_count=0;
		let tag;
		loop{
			match ctx.redis.get_del::<&String,String>(&etag).await{
				Ok(etag)=>{
					if etag.is_empty(){
						return err_handle(&ctx,&session).await;
					}
					tag=etag;
					break;
				},
				Err(e)=>{
					error_count+=1;
					if error_count>10*60{//10分間毎秒確認
						eprintln!("{}:{} {:?}",file!(),line!(),e);
						return err_handle(&ctx,&session).await;
					}else{
						tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
					}
				}
			}
		}
		parts.push(s3::serde_types::Part{
			part_number,
			etag:tag,
		});
		part_number+=1;
	}
	if let Some(n)=session.part_number{
		if part_number!=n+2{
			eprintln!("{}:{} {}!={}",file!(),line!(),part_number,n+2);
			return (StatusCode::BAD_REQUEST).into_response();
		}
	}else{
		eprintln!("{}:{}",file!(),line!());
		return (StatusCode::BAD_REQUEST).into_response();
	}
	let md5sum=crate::md5_ontext_from_raw(&session.md5_ctx_64);
	let md5sum=md5sum.compute().0;
	let md5sum = md5sum.iter().map(|n| format!("{:02x}", n)).collect::<String>();
	let cache_control="max-age=31536000, immutable";
	let detected_name=percent_encoding::percent_encode(session.name.as_bytes(), percent_encoding::NON_ALPHANUMERIC);
	let content_disposition=format!("inline; filename=\"{}\"",detected_name);
	if session.upload_id.is_none(){
		return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
	}
	match ctx.bucket.complete_multipart_upload_with_metadata(&session.s3_key,&session.upload_id.unwrap(),parts,Some(&cache_control),Some(&content_disposition)).await{
		Ok(_resp) => {},
		Err(e) =>{
			println!("{:?} \n{}",session.part_etag,session.content_length);
			eprintln!("{}:{} {:?}",file!(),line!(),e);
			return (axum::http::StatusCode::INTERNAL_SERVER_ERROR).into_response();
		},
	}
	let mut thumbnail_key=None;
	let mut width=0;
	let mut height=0;
	let mut blurhash=None;
	let mut maybe_sensitive=false;
	if session.content_type.starts_with("video/"){
		//let start_time=chrono::Utc::now();
		if let Some(info)=ctx.file_service.ffmpeg_metadata(&ctx.config,&session.s3_key,2048,session.sensitive_threshold,session.skip_sensitive_detection).await{
			width=info.width;
			height=info.height;
			blurhash=info.blurhash;
			maybe_sensitive=info.maybe_sensitive.unwrap_or_default();

			let thumbnail_bin=info.thumbnail.as_ref();
			thumbnail_key=match thumbnail_bin{
				Some(thumbnail_bin)=>{
					let cache_control="max-age=31536000, immutable";
					let detected_name=percent_encoding::percent_encode(session.name.as_bytes(), percent_encoding::NON_ALPHANUMERIC);
					let content_disposition=format!("inline; filename=\"{}\"",detected_name);
					let thumbnail_key=format!("{}/thumbnail-{}{}",ctx.config.prefix,uuid::Uuid::new_v4().to_string(),".webp");
					match ctx.bucket.put_object_with_metadata(&thumbnail_key,&thumbnail_bin,"image/webp",s3::command::ContentMd5::Auto,cache_control,&content_disposition).await{
						Ok(_)=>Some(thumbnail_key),
						Err(_)=>None,
					}
				},
				None=>None
			};
		}
		//println!("thumbnail {}ms",(chrono::Utc::now()-start_time).num_milliseconds());
	}
	let user=match ctx.raw_db.get().await{
		Some(mut con)=>MiUser::load_by_id(&mut con, &session.user_id).await,
		None=>None
	};
	let user=match user{
		Some(u)=>u,
		None=>{
			eprintln!("{}:{} NoUser",file!(),line!());
			return (axum::http::StatusCode::INTERNAL_SERVER_ERROR).into_response();
		}
	};
	let res=ctx.drive_service.register_file(
		Some(&user),
		session.s3_key.as_str(),
		session.folder_id.as_deref(),
		session.comment.as_deref(),
		blurhash.as_deref(),
		false,
		width,
		height,
		maybe_sensitive,
		"",
		session.is_sensitive,
		None,
		None,
		session.name,
		md5sum,
		session.content_type.to_owned(),
		session.content_length as i64,
		session.force,
		thumbnail_key.as_deref(),
		ctx.config.public_base_url.clone(),
	).await;
	if let None=res{
		return (axum::http::StatusCode::BAD_REQUEST).into_response();
	}
	let (_file,res)=res.unwrap();
	let mut header=axum::http::header::HeaderMap::new();
	header.insert(axum::http::header::CONTENT_TYPE,"application/json".parse().unwrap());
	let status=axum::http::StatusCode::OK;
	let res=serde_json::to_string(&res.unwrap_or(serde_json::Value::Null)).unwrap_or_default();
	(status,header,res).into_response()
}
