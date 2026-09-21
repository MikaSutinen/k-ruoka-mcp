//! Recipe search: `GET /kr-api/v1/search`.
//!
//! Read-only. The response already carries each recipe's ingredients with the EAN of
//! the product K-Ruoka links to it, so no second call is needed to turn a recipe into
//! something `add_to_cart` accepts.

use crate::browser::catalog::percent_encode;
use crate::browser::session::{ApiError, KrApi};
use crate::types::{RecipeSearchResponse, RecipeSearchView};

/// A full page of recipes with ingredients is over 100 KB, far more than a model can
/// use, so the tool asks for and returns far fewer.
const MAX_LIMIT: u32 = 10;
const DEFAULT_LIMIT: u32 = 5;

pub struct Recipes<'a> {
    api: &'a dyn KrApi,
}

impl<'a> Recipes<'a> {
    pub fn new(api: &'a dyn KrApi) -> Self {
        Self { api }
    }

    /// Search recipes by dish name or ingredient, in Finnish.
    pub async fn search(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<RecipeSearchView, ApiError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(ApiError::InvalidRequest(
                "query must not be empty. Pass a dish name, e.g. \"makaronilaatikko\".".to_string(),
            ));
        }
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let path = format!(
            "/kr-api/v1/search?q={}&offset=0&limit={limit}",
            percent_encode(query)
        );
        let value = self.api.call("GET", &path, None).await?;
        let response: RecipeSearchResponse = serde_json::from_value(value)
            .map_err(|e| ApiError::Other(anyhow::anyhow!("unexpected recipe search shape: {e}")))?;
        Ok(RecipeSearchView::from_response(response, limit as usize))
    }
}
