//! `search_recipes` and `save_cart_as_list`, over the in-process MCP connection against
//! the fake K-Ruoka.
//!
//! As elsewhere, the assertions on `api.calls()` check the requests actually sent, and
//! the "deaf" cases model K-Ruoka answering 200 without applying a change.

mod support;

use serde_json::json;
use support::{BANANA, BASKET_ID, LIST_ID, MockApi, STORE, call_tool, connect, try_call_tool};

fn list_calls(api: &MockApi) -> Vec<support::Call> {
    api.calls()
        .into_iter()
        .filter(|c| c.path.starts_with("/kr-api/shopping-lists/"))
        .collect()
}

// ---------------------------------------------------------------------------
// search_recipes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recipes_come_back_compact_with_ingredient_eans() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new()).await?;

    let found = call_tool(
        &client,
        "search_recipes",
        json!({"query": "makaroni", "limit": 2}),
    )
    .await
    .unwrap();

    assert_eq!(found["totalHits"], 16);
    let recipes = found["recipes"].as_array().unwrap();
    assert_eq!(
        recipes.len(),
        2,
        "limit applies to the result as well as the request"
    );
    assert_eq!(recipes[0]["name"], "Makaronilaatikko");
    assert_eq!(recipes[0]["servings"], "4 annosta");
    assert_eq!(recipes[0]["prepTime"], "30 - 60 min");

    let ingredients = recipes[0]["ingredients"].as_array().unwrap();
    assert_eq!(ingredients[0]["ean"], "6410405327888");
    assert_eq!(ingredients[1]["amount"], "1/2");
    assert_eq!(ingredients[1]["unit"], "dl");
    assert!(
        ingredients[2].get("ean").is_none(),
        "an ingredient with no product has no ean"
    );
    assert_eq!(ingredients[3]["isAlternative"], true);
    assert_eq!(ingredients[0]["isAlternative"], false);

    let sent = api.calls();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "GET");
    assert_eq!(
        sent[0].path,
        "/kr-api/v1/search?q=makaroni&offset=0&limit=2"
    );
    Ok(())
}

#[tokio::test]
async fn recipe_query_is_escaped_and_limit_is_capped() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new()).await?;

    call_tool(
        &client,
        "search_recipes",
        json!({"query": "makaroni & juusto", "limit": 999}),
    )
    .await
    .unwrap();

    assert_eq!(
        api.calls()[0].path,
        "/kr-api/v1/search?q=makaroni%20%26%20juusto&offset=0&limit=10"
    );
    Ok(())
}

#[tokio::test]
async fn empty_recipe_query_is_refused_without_a_request() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new()).await?;

    let err = call_tool(&client, "search_recipes", json!({"query": "   "}))
        .await
        .unwrap_err();

    assert!(err.contains("must not be empty"), "{err}");
    assert!(api.calls().is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// save_cart_as_list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn saves_renames_and_shares_in_that_order() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new().with_item(BANANA, 5.0, "kpl")).await?;

    let saved = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": " Kauppalista 21.9 ", "share_with_household": true}),
    )
    .await
    .unwrap();

    assert_eq!(saved["listId"], LIST_ID);
    assert_eq!(saved["name"], "Kauppalista 21.9");
    assert_eq!(saved["itemsInCart"], 1);
    assert_eq!(saved["itemsOnList"], 1);
    assert_eq!(saved["sharedWithHousehold"], true);

    let lists = list_calls(&api);
    assert_eq!(lists.len(), 3);
    assert_eq!(lists[0].method, "POST");
    assert_eq!(
        lists[0].path,
        format!("/kr-api/shopping-lists/from/basket/{BASKET_ID}?storeId={STORE}")
    );
    assert!(lists[0].body.is_none());
    assert_eq!(lists[1].method, "PATCH");
    assert_eq!(
        lists[1].path,
        format!("/kr-api/shopping-lists/{LIST_ID}?storeId={STORE}")
    );
    assert_eq!(lists[1].body, Some(json!({"name": "Kauppalista 21.9"})));
    assert_eq!(
        lists[2].path,
        format!("/kr-api/shopping-lists/{LIST_ID}/share")
    );
    assert_eq!(
        lists[2].body,
        Some(json!({
            "canReadWithShareAccess": false,
            "canWriteWithShareAccess": false,
            "canWriteWithHouseholdAccess": true,
        }))
    );
    Ok(())
}

#[tokio::test]
async fn sharing_is_off_unless_asked_for() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new().with_item(BANANA, 1.0, "kpl")).await?;

    let saved = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "Kauppalista"}),
    )
    .await
    .unwrap();

    assert_eq!(saved["sharedWithHousehold"], false);
    assert!(list_calls(&api).iter().all(|c| !c.path.ends_with("/share")));
    Ok(())
}

#[tokio::test]
async fn an_empty_cart_is_refused_before_any_list_is_made() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new()).await?;

    let err = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "Kauppalista"}),
    )
    .await
    .unwrap_err();

    assert!(err.contains("cart is empty"), "{err}");
    assert!(list_calls(&api).is_empty());
    Ok(())
}

#[tokio::test]
async fn a_blank_name_is_refused_before_any_request() -> anyhow::Result<()> {
    let (client, api) = connect(MockApi::new().with_item(BANANA, 1.0, "kpl")).await?;

    let err = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "  "}),
    )
    .await
    .unwrap_err();

    assert!(err.contains("name must not be empty"), "{err}");
    assert!(api.calls().is_empty());
    Ok(())
}

/// K-Ruoka answers 200 to a rename it did not apply. The tool must notice, name the
/// half-made list, and not go on to share it.
#[tokio::test]
async fn an_ignored_rename_is_reported_with_the_list_id() -> anyhow::Result<()> {
    let api = MockApi::new()
        .with_item(BANANA, 1.0, "kpl")
        .deaf_to("RENAME-LIST");
    let (client, api) = connect(api).await?;

    let err = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "Kauppalista 21.9", "share_with_household": true}),
    )
    .await
    .unwrap_err();

    assert!(err.contains(LIST_ID), "{err}");
    assert!(err.contains("renaming it"), "{err}");
    assert!(err.contains("Ostoskori"), "{err}");
    assert!(list_calls(&api).iter().all(|c| !c.path.ends_with("/share")));
    Ok(())
}

#[tokio::test]
async fn an_ignored_share_is_reported() -> anyhow::Result<()> {
    let api = MockApi::new()
        .with_item(BANANA, 1.0, "kpl")
        .deaf_to("SHARE-LIST");
    let (client, _) = connect(api).await?;

    let err = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "Kauppalista 21.9", "share_with_household": true}),
    )
    .await
    .unwrap_err();

    assert!(err.contains("sharing it"), "{err}");
    assert!(err.contains(LIST_ID), "{err}");
    assert!(err.contains("Kauppalista 21.9"), "{err}");
    Ok(())
}

#[tokio::test]
async fn a_failing_share_call_names_the_list_that_now_exists() -> anyhow::Result<()> {
    let api = MockApi::new()
        .with_item(BANANA, 1.0, "kpl")
        .failing_on("/share", "boom");
    let (client, _) = connect(api).await?;

    let failure = try_call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "Kauppalista 21.9", "share_with_household": true}),
    )
    .await
    .unwrap_err();

    // A tool failure the model can read, not a protocol error.
    assert!(matches!(failure, support::Failure::ToolError(_)));
    assert!(failure.text().contains(LIST_ID), "{}", failure.text());
    assert!(failure.text().contains("sharing it"), "{}", failure.text());
    // The rename already worked, so the list is no longer called "Ostoskori".
    assert!(failure.text().contains("Kauppalista 21.9"), "{}", failure.text());
    assert!(!failure.text().contains("Ostoskori"), "{}", failure.text());
    Ok(())
}

#[tokio::test]
async fn a_lost_creation_response_is_reported_as_uncertain() -> anyhow::Result<()> {
    let api = MockApi::new()
        .with_item(BANANA, 1.0, "kpl")
        .failing_on("/from/basket/", "gone");
    let (client, api) = connect(api).await?;

    let err = call_tool(
        &client,
        "save_cart_as_list",
        json!({"store_id": STORE, "name": "Kauppalista 21.9", "share_with_household": true}),
    )
    .await
    .unwrap_err();

    assert!(err.contains("may already have been created"), "{err}");
    assert!(err.contains("not repeated"), "{err}");
    let posts = list_calls(&api)
        .iter()
        .filter(|c| c.method == "POST")
        .count();
    assert_eq!(posts, 1);
    Ok(())
}
