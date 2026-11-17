//! Explorateur d'API : appelle n'importe quelle route JSON du catalogue `fs_api::routes`.
//!
//! Le front envoie un identifiant de route, jamais un chemin. Les routes destructives exigent
//! `confirmed = true` (le front affiche d'abord une confirmation).

use std::collections::BTreeMap;

use fs_api::routes::{self, Risk, Route};
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::{client, AppState};

/// Vérifie qu'une route peut être appelée avec ce niveau de confirmation.
pub fn check_call(route: &Route, confirmed: bool) -> Result<(), String> {
    if !route.callable() {
        return Err(format!(
     "« {} » n'est pas appelable ici (flux, fichier ou binaire)",
     route.id
        ));
    }
    if route.risk == Risk::Destructive && !confirmed {
        return Err(format!("« {} » est destructive : confirmation requise", route.id));
    }
    Ok(())
}

/// Catalogue complet, pour construire l'UI.
#[tauri::command]
pub fn api_routes() -> Vec<Route> {
    routes::ROUTES.to_vec()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn api_call(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path_params: Option<BTreeMap<String, String>>,
    query: Option<Vec<(String, String)>>,
    headers: Option<Vec<(String, String)>>,
    body: Option<Value>,
    confirmed: Option<bool>,
) -> Result<Value, String> {
    let route = routes::find(&id).ok_or_else(|| format!("route inconnue : {id}"))?;
    check_call(route, confirmed.unwrap_or(false))?;
    let c = client(&app, &state).await?;
    c.call_route(
 route,
        &path_params.unwrap_or_default(),
        &query.unwrap_or_default(),
        &headers.unwrap_or_default(),
        body,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_needs_confirmation() {
        let del = routes::find("chats.delete").unwrap();
        assert!(check_call(del, false).is_err());
        assert!(check_call(del, true).is_ok());
        let list = routes::find("chats.list").unwrap();
 assert!(check_call(list, false).is_ok());
    }

    #[test]
    fn non_json_routes_are_refused() {
        for id in ["chats.create_stream", "files.upload", "skills.content", "chats.attach"] {
     let r = routes::find(id).unwrap();
     assert!(check_call(r, true).is_err(), "{id}");
        }
    }

    #[test]
    fn every_callable_route_is_reachable() {
 let n = routes::ROUTES.iter().filter(|r| r.callable()).count();
        assert!(n > 90, "{n}");
    }
}
