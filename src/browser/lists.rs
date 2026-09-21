//! Saved shopping lists: turning the cart into a named list, optionally shared with the
//! household.
//!
//! Three calls, in order: create the list from the cart, rename it, share it. K-Ruoka
//! names the new list "Ostoskori" and offers no way to name it at creation, and it
//! answers `200` whether or not a follow-up change took, so each step is checked against
//! the list it returns rather than against the status.

use crate::browser::basket::Cart;
use crate::browser::catalog::percent_encode;
use crate::browser::session::{ApiError, KrApi};
use crate::types::{SavedListView, ShareSettings, ShoppingList};

pub struct Lists<'a> {
    api: &'a dyn KrApi,
}

impl<'a> Lists<'a> {
    pub fn new(api: &'a dyn KrApi) -> Self {
        Self { api }
    }

    /// Save the cart at `store_id` as a list called `name`.
    ///
    /// The basket id is read fresh from the server, never remembered: it is what decides
    /// which cart becomes the list. An empty cart is refused, since an empty list is
    /// never what a caller meant. If the list is created but a later step fails, the
    /// error says so and names the list, because there is no call here to undo it.
    pub async fn save_cart(
        &self,
        store_id: &str,
        name: &str,
        share_with_household: bool,
    ) -> Result<SavedListView, ApiError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::InvalidRequest(
                "name must not be empty. Pass the list name, e.g. \"Kauppalista 21.9\"."
                    .to_string(),
            ));
        }
        let basket = Cart::new(self.api).active(store_id).await?;
        if basket.items.is_empty() {
            return Err(ApiError::InvalidRequest(
                "the cart is empty, so there is nothing to save as a list. Add items first."
                    .to_string(),
            ));
        }

        let store = percent_encode(store_id);
        let path = format!(
            "/kr-api/shopping-lists/from/basket/{}?storeId={store}",
            percent_encode(&basket.id)
        );
        // Never replayed: a second attempt after a lost response could make a second list.
        let response = self
            .api
            .call_once("POST", &path, None)
            .await
            .map_err(creation_may_have_succeeded)?;
        let created: ShoppingList = parse(response)?;
        if created.id.is_empty() {
            return Err(ApiError::Other(anyhow::anyhow!(
                "K-Ruoka answered without a list id, so the list may not have been created"
            )));
        }
        let list_path = format!(
            "/kr-api/shopping-lists/{}?storeId={store}",
            percent_encode(&created.id)
        );

        let body = serde_json::json!({ "name": name });
        let renamed: ShoppingList = self
            .step(
                &created,
                "renaming it",
                self.api.call("PATCH", &list_path, Some(&body)),
            )
            .await?;
        if renamed.name != name {
            return Err(partial(
                &created,
                "renaming it",
                &format!(
                    "K-Ruoka answered 200 but the list is named {:?}",
                    renamed.name
                ),
            ));
        }

        let mut shared = false;
        if share_with_household {
            let path = format!(
                "/kr-api/shopping-lists/{}/share",
                percent_encode(&created.id)
            );
            let body = serde_json::json!({
                "canReadWithShareAccess": false,
                "canWriteWithShareAccess": false,
                "canWriteWithHouseholdAccess": true,
            });
            let settings: ShareSettings = self
                .step(
                    &renamed,
                    "sharing it",
                    self.api.call("PATCH", &path, Some(&body)),
                )
                .await?;
            if !settings.can_write_with_household_access {
                return Err(partial(
                    &renamed,
                    "sharing it",
                    "K-Ruoka answered 200 but the household still cannot edit the list",
                ));
            }
            shared = true;
        }

        Ok(SavedListView {
            list_id: renamed.id,
            name: renamed.name,
            items_in_cart: basket.items.len(),
            items_on_list: created.items.len(),
            shared_with_household: shared,
        })
    }

    /// Await one follow-up call and parse it, turning any failure into a message that
    /// names the half-made list.
    async fn step<T: serde::de::DeserializeOwned>(
        &self,
        created: &ShoppingList,
        what: &str,
        call: impl std::future::Future<Output = Result<serde_json::Value, ApiError>>,
    ) -> Result<T, ApiError> {
        let value = call
            .await
            .map_err(|e| partial(created, what, &e.to_string()))?;
        serde_json::from_value(value)
            .map_err(|e| partial(created, what, &format!("unexpected response shape: {e}")))
    }
}

/// A failure where the server may or may not have acted. The request was not repeated, so
/// the list could exist even though this call reports failure.
fn creation_may_have_succeeded(error: ApiError) -> ApiError {
    match error {
        ApiError::BrowserGone { .. } | ApiError::Cloudflare { .. } => {
            ApiError::Other(anyhow::anyhow!(
                "{error}. The list may already have been created and the request was not \
                 repeated: check the shopping lists in the K-Ruoka account before trying again."
            ))
        }
        other => other,
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, ApiError> {
    serde_json::from_value(value)
        .map_err(|e| ApiError::Other(anyhow::anyhow!("unexpected shopping list shape: {e}")))
}

fn partial(list: &ShoppingList, what: &str, cause: &str) -> ApiError {
    ApiError::Other(anyhow::anyhow!(
        "the list was created (id {}, currently named {:?}) but {what} failed: {cause}. \
         It is still in the account and can be fixed by hand.",
        list.id,
        list.name
    ))
}
