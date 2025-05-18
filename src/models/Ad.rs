use crate::DBConnection;
use diesel::{ExpressionMethods, QueryDsl, Selectable, SelectableHelper};
use diesel_async::RunQueryDsl;

diesel::table! {
	#[sql_name = "ad"]
	clip (id) {
		id -> VarChar,
		expiresAt -> Timestamp,
		startsAt -> Timestamp,

		place -> VarChar,
		priority -> VarChar,

		ratio -> Int4,
		url -> VarChar,
		imageUrl -> VarChar,
		memo -> VarChar,

		dayOfWeek -> Int4,
	}
}
#[derive(
    PartialEq,
    Eq,
    Debug,
    Clone,
    diesel::Insertable,
    diesel::Queryable,
    Selectable,
    diesel::QueryableByName,
)]
#[diesel(table_name = ad)]
pub struct MiAd {
    pub id: String,
    #[diesel(column_name = "expiresAt")]
    pub expires_at: NaiveDateTime,
    #[diesel(column_name = "startsAt")]
    pub starts_at: NaiveDateTime,
    #[diesel(column_name = "place")]
    pub place: String,
    #[diesel(column_name = "priority")]
    pub priority: String,
    #[diesel(column_name = "ratio")]
    pub ratio: i32,
    #[diesel(column_name = "url")]
    pub url: String,
    #[diesel(column_name = "imageUrl")]
    pub image_url: String,
    #[diesel(column_name = "memo")]
    pub memo: String,
    #[diesel(column_name = "dayOfWeek")]
    pub day_of_week: i32,
}
impl MiAd {
}
