use diesel::{ExpressionMethods, QueryDsl, Selectable, SelectableHelper};
use diesel_async::RunQueryDsl;

diesel::table! {
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
    pub description: String,
    #[diesel(column_name = "lastFetchedAt")]
    pub last_fetched_at: Option<NaiveDateTime>,
    pub uri: Option<String>,
}
