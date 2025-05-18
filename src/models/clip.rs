use crate::DBConnection;
use diesel::{ExpressionMethods, QueryDsl, Selectable, SelectableHelper};
use diesel_async::RunQueryDsl;

diesel::table! {
	#[sql_name = "clip"]
	clip (id) {
		id -> VarChar,
		userId -> VarChar,
		name -> VarChar,
		isPublic -> Boolean,
		description -> Nullable<VarChar>,
		lastFetchedAt -> Nullable<Timestamp>,
		uri -> Nullable<VarChar>,
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
#[diesel(table_name = clip)]
pub struct MiClip {
    pub id: String,
    #[diesel(column_name = "userId")]
    pub user_id: String,
    #[diesel(column_name = "isPublic")]
    pub is_public: String,
    #[diesel(column_name = "description")]
    pub description: String,
    #[diesel(column_name = "lastFetchedAt")]
    pub last_fetched_at: Option<NaiveDateTime>,
    #[diesel(column_name = "uri")]
    pub uri: Option<String>,
}
impl MiClip {
}
