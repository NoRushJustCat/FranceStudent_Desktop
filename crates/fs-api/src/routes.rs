//! Catalogue des routes de l'API France Student (source : `API_FranceStudent.md`).
//!
//! Chaque route a un identifiant stable, une méthode, un chemin à `{paramètres}`, un niveau de
//! risque et les paramètres de requête / en-têtes documentés. `Client::call_route` n'accepte que
//! ce qui est déclaré ici : pas de chemin libre, pas de paramètre inconnu.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Lecture seule.
    Read,
    /// Modifie des données, réversible ou peu sensible.
    Write,
    /// Suppression, révocation, consommation de crédit : confirmation obligatoire.
    Destructive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Corps et réponse JSON (ou texte) : appelable via `call_route`.
    Json,
    /// `multipart/form-data` : passe par une commande dédiée.
    Multipart,
    /// `text/event-stream` : passe par le moteur de chat.
    Stream,
    /// Réponse binaire (ZIP, image, JSON téléchargé) : passe par une commande dédiée.
    Binary,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub group: &'static str,
    pub summary: &'static str,
    pub risk: Risk,
    pub kind: Kind,
    pub query: &'static [&'static str],
    pub headers: &'static [&'static str],
    pub has_body: bool,
}

const fn r(
    id: &'static str,
    method: &'static str,
    path: &'static str,
    group: &'static str,
    summary: &'static str,
) -> Route {
    let risk = if matches!(method.as_bytes(), b"GET") {
        Risk::Read
    } else {
        Risk::Write
    };
    Route {
        id,
        method,
        path,
 group,
        summary,
        risk,
 kind: Kind::Json,
        query: &[],
        headers: &[],
 has_body: false,
    }
}

impl Route {
    const fn q(self, query: &'static [&'static str]) -> Self {
        Route { query, ..self }
    }
    const fn h(self, headers: &'static [&'static str]) -> Self {
        Route { headers, ..self }
    }
    const fn body(self) -> Self {
        Route {
            has_body: true,
     ..self
        }
    }
    const fn kind(self, kind: Kind) -> Self {
        Route { kind, ..self }
    }
    const fn danger(self) -> Self {
        Route {
     risk: Risk::Destructive,
            ..self
 }
    }

    /// Noms des `{paramètres}` de chemin, dans l'ordre.
    pub fn path_params(&self) -> Vec<&'static str> {
 let mut out = Vec::new();
 let mut rest = self.path;
 while let Some(i) = rest.find('{') {
     let after = &rest[i + 1..];
     match after.find('}') {
         Some(j) => {
      out.push(&after[..j]);
      rest = &after[j + 1..];
         }
         None => break,
     }
 }
        out
    }

    /// Chemin avec paramètres encodés. Refuse tout paramètre manquant, vide ou inconnu.
    pub fn build_path(&self, params: &BTreeMap<String, String>) -> Result<String, String> {
 let names = self.path_params();
        for k in params.keys() {
     if !names.contains(&k.as_str()) {
         return Err(format!("paramètre de chemin inconnu : {k}"));
     }
        }
 let mut out = String::new();
        let mut rest = self.path;
        while let Some(i) = rest.find('{') {
     out.push_str(&rest[..i]);
     let after = &rest[i + 1..];
     let j = after.find('}').ok_or("chemin mal formé")?;
     let name = &after[..j];
     let val = params
         .get(name)
         .map(|v| v.trim())
  .filter(|v| !v.is_empty())
  .ok_or_else(|| format!("paramètre de chemin manquant : {name}"))?;
     out.push_str(&encode_segment(val));
     rest = &after[j + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Vérifie que les paramètres de requête et en-têtes sont documentés pour cette route.
    pub fn check_extras(&self, query: &[(String, String)], headers: &[(String, String)]) -> Result<(), String> {
        for (k, _) in query {
     if !self.query.contains(&k.as_str()) {
         return Err(format!("paramètre de requête inconnu : {k}"));
     }
        }
        for (k, _) in headers {
     if !self.headers.iter().any(|h| h.eq_ignore_ascii_case(k)) {
         return Err(format!("en-tête non autorisé : {k}"));
     }
        }
        Ok(())
    }

    /// Une route que `call_route` peut exécuter telle quelle.
    pub fn callable(&self) -> bool {
        self.kind == Kind::Json
    }
}

/// Encodage pourcentage d'un segment : tout sauf `A-Za-z0-9-._~`. Interdit donc `/`, `?`, `#`, `..`.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
 if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
     out.push(b as char);
        } else {
     out.push_str(&format!("%{b:02X}"));
        }
    }
    // « .. » et « . » seuls restent des segments spéciaux pour certains serveurs.
    if out == "." || out == ".." {
        out = out.replace('.', "%2E");
    }
    out
}

pub fn find(id: &str) -> Option<&'static Route> {
    ROUTES.iter().find(|r| r.id == id)
}

const LOCK: &[&str] = &["X-Locked-Chats-Access"];
const ORIGIN: &[&str] = &["Origin"];
const PAGE: &[&str] = &["limit", "cursor_chat_id"];
const SKILL_Q: &[&str] = &["after", "limit", "order"];

pub static ROUTES: &[Route] = &[
    // ----- Chats -----
    r("chats.list", "GET", "/chats", "Chats", "Lister les chats").q(&["limit", "cursor_chat_id", "sortedBy"]),
    r("chats.delete_all", "DELETE", "/chats", "Chats", "Supprimer tous les chats").danger(),
    r("chats.create_stream", "POST", "/chats/stream", "Chats", "Créer un chat et streamer").body().kind(Kind::Stream),
    r("chats.get", "GET", "/chats/{chatId}", "Chats", "Récupérer un chat"),
    r("chats.send", "POST", "/chats/{chatId}", "Chats", "Envoyer un message (stream)").body().kind(Kind::Stream),
    r("chats.update", "PATCH", "/chats/{chatId}", "Chats", "Mettre à jour un chat").body(),
    r("chats.delete", "DELETE", "/chats/{chatId}", "Chats", "Supprimer un chat").danger(),
    r("chats.attach", "GET", "/chats/{chatId}/stream", "Chats", "Se rattacher au stream").q(&["last_event_id"]).kind(Kind::Stream),
    r("chats.cancel", "POST", "/chats/{chatId}/stream/cancel", "Chats", "Arrêter la génération"),
    r("chats.edit_message", "PATCH", "/chats/{chatId}/messages/{messageId}", "Chats", "Éditer un message (fork)").body(),
    r("chats.context", "GET", "/chats/{chatId}/context", "Chats", "Occupation du contexte"),
    r("chats.compact", "POST", "/chats/{chatId}/context", "Chats", "Compacter le contexte"),
    r("chats.usage", "GET", "/chats/{chatId}/usage", "Chats", "Usage d'un chat"),
    r("chats.pin", "PUT", "/chats/{chatId}/pins", "Chats", "Épingler"),
    r("chats.unpin", "DELETE", "/chats/{chatId}/pins", "Chats", "Désépingler"),
    r("chats.archive", "PUT", "/chats/{chatId}/archive", "Chats", "Archiver"),
    r("chats.restore", "PUT", "/chats/{chatId}/restore", "Chats", "Restaurer un chat archivé"),
    r("chats.archive_download", "GET", "/chats/{chatId}/archive/download", "Chats", "Télécharger les messages archivés").kind(Kind::Binary),
    r("chats.archived", "GET", "/chats/archived", "Chats", "Lister les chats archivés").q(PAGE),
    r("chats.search", "GET", "/chats/search", "Chats", "Rechercher dans les chats").q(&["query", "limit"]),
    r("chats.agents", "GET", "/chats/agents", "Chats", "Lister les agents").q(&["include-disabled"]),
    r("chats.shares_list", "GET", "/chats/{chatId}/shares", "Partages de chat", "Lister les partages"),
    r("chats.shares_add", "POST", "/chats/{chatId}/shares", "Partages de chat", "Ajouter un partage").body(),
    r("chats.shares_update", "PATCH", "/chats/{chatId}/shares", "Partages de chat", "Modifier un partage").body(),
    r("chats.shares_remove", "DELETE", "/chats/{chatId}/shares", "Partages de chat", "Supprimer un partage").body().danger(),
    r("chats.image", "GET", "/chats/{chatId}/messages/{messageId}/images/{imageId}", "Chats", "Image générée").kind(Kind::Binary),
    r("messages.vote", "PATCH", "/messages/{messageId}", "Messages", "Voter pour un message").body(),
    // ----- OpenAI compatible, modèles -----
    r("openai.responses", "POST", "/v1/responses", "OpenAI compatible", "Créer une réponse").body().kind(Kind::Stream),
    r("openai.models", "GET", "/v1/models", "OpenAI compatible", "Lister les modèles (format OpenAI)"),
    r("models.list", "GET", "/models", "Modèles", "Lister les modèles"),
    // ----- Projets -----
    r("projects.list", "GET", "/projects", "Projets", "Lister mes projets"),
    r("projects.create", "POST", "/projects", "Projets", "Créer un projet").body(),
    r("projects.shared", "GET", "/projects/shared", "Projets", "Projets partagés avec moi"),
    r("projects.get", "GET", "/projects/{projectId}", "Projets", "Récupérer un projet"),
    r("projects.update", "PATCH", "/projects/{projectId}", "Projets", "Modifier un projet").body(),
    r("projects.delete", "DELETE", "/projects/{projectId}", "Projets", "Supprimer un projet").danger(),
    r("projects.chats", "GET", "/projects/{projectId}/chats", "Projets", "Chats d'un projet").q(PAGE),
    r("projects.attach_chat", "PUT", "/projects/{projectId}/chats/{chatId}", "Projets", "Rattacher un chat"),
    r("projects.detach_chat", "DELETE", "/projects/{projectId}/chats/{chatId}", "Projets", "Retirer un chat du projet").danger(),
    r("projects.shares_list", "GET", "/projects/{projectId}/shares", "Partages de projet", "Lister les partages"),
    r("projects.shares_add", "POST", "/projects/{projectId}/shares", "Partages de projet", "Ajouter un partage").body(),
    r("projects.shares_remove", "DELETE", "/projects/{projectId}/shares/{targetUserId}", "Partages de projet", "Retirer un partage").danger(),
    // ----- Chats verrouillés -----
    r("locked.status", "GET", "/locked-chats/status", "Chats verrouillés", "PIN créé ?"),
    r("locked.pin_create", "POST", "/locked-chats/pin", "Chats verrouillés", "Créer le PIN").body(),
    r("locked.pin_change", "PUT", "/locked-chats/pin", "Chats verrouillés", "Changer le PIN").body(),
    r("locked.pin_reset", "DELETE", "/locked-chats/pin", "Chats verrouillés", "Réinitialiser le PIN (supprime les chats verrouillés)").danger(),
    r("locked.access_open", "POST", "/locked-chats/access", "Chats verrouillés", "Ouvrir l'accès").body(),
    r("locked.access_close", "DELETE", "/locked-chats/access", "Chats verrouillés", "Verrouiller maintenant").h(LOCK),
    r("locked.list", "GET", "/locked-chats", "Chats verrouillés", "Lister les chats verrouillés").h(LOCK),
    r("locked.lock_chat", "PUT", "/locked-chats/chats/{chatId}", "Chats verrouillés", "Verrouiller un chat").h(LOCK),
    r("locked.unlock_chat", "DELETE", "/locked-chats/chats/{chatId}", "Chats verrouillés", "Déverrouiller un chat").h(LOCK).danger(),
    // ----- Fichiers -----
    r("files.upload", "POST", "/files/upload", "Fichiers", "Uploader (historique)").kind(Kind::Multipart),
    r("files.list", "GET", "/files", "Fichiers", "Lister (historique)"),
    r("files.get", "GET", "/files/{fileId}", "Fichiers", "Récupérer (historique)"),
    r("files.delete", "DELETE", "/files/{fileId}", "Fichiers", "Supprimer (historique)").danger(),
    r("filesv2.list", "GET", "/filesv2", "Files v2", "Lister les fichiers").q(&["after", "limit", "order", "purpose"]),
    r("filesv2.upload", "POST", "/filesv2", "Files v2", "Uploader un fichier").q(&["expires_after[anchor]", "expires_after[seconds]"]).kind(Kind::Multipart),
    r("filesv2.get", "GET", "/filesv2/{file_id}", "Files v2", "Récupérer un fichier"),
    r("filesv2.delete", "DELETE", "/filesv2/{file_id}", "Files v2", "Supprimer un fichier").danger(),
    r("filesv2.content", "GET", "/filesv2/{file_id}/content", "Files v2", "Contenu d'un fichier").kind(Kind::Binary),
    r("filesv2.usage", "GET", "/filesv2/usage", "Files v2", "Usage du stockage"),
    r("filesv2.mime", "GET", "/filesv2/supported-mime-types", "Files v2", "Types MIME supportés"),
    r("filesv2.migration", "GET", "/filesv2/legacy-migration", "Files v2", "Statut de la migration"),
    r("filesv2.migrate", "POST", "/filesv2/legacy-migration", "Files v2", "Migrer les anciens fichiers"),
    // ----- Skills -----
    r("skills.list", "GET", "/skills", "Skills", "Lister les skills").q(&["after", "limit", "order", "query", "flag", "flags", "flags_bitfield"]),
    r("skills.create", "POST", "/skills", "Skills", "Créer un skill").q(&["flags"]).kind(Kind::Multipart),
    r("skills.shared", "GET", "/skills/shared", "Skills", "Skills partagés avec moi").q(&["query"]),
    r("skills.get", "GET", "/skills/{skillId}", "Skills", "Récupérer un skill"),
    r("skills.update", "POST", "/skills/{skillId}", "Skills", "Modifier un skill").body(),
    r("skills.delete", "DELETE", "/skills/{skillId}", "Skills", "Supprimer un skill").danger(),
    r("skills.content", "GET", "/skills/{skillId}/content", "Skills", "ZIP du skill").kind(Kind::Binary),
    r("skills.versions", "GET", "/skills/{skillId}/versions", "Skills", "Lister les versions").q(SKILL_Q),
    r("skills.version_create", "POST", "/skills/{skillId}/versions", "Skills", "Créer une version").q(&["default"]).kind(Kind::Multipart),
    r("skills.version_delete", "DELETE", "/skills/{skillId}/versions/{version}", "Skills", "Supprimer une version").danger(),
    r("skills.version_content", "GET", "/skills/{skillId}/versions/{version}/content", "Skills", "ZIP d'une version").kind(Kind::Binary),
    r("skills.shares_list", "GET", "/skills/{skillId}/shares", "Skills", "Lister les partages"),
    r("skills.shares_add", "POST", "/skills/{skillId}/shares", "Skills", "Partager (10 max)").body(),
    r("skills.shares_remove", "DELETE", "/skills/{skillId}/shares/{targetUserId}", "Skills", "Retirer un partage").danger(),
    r("skills.shared_update", "POST", "/skills/shared/{skillId}", "Skills", "Activer/désactiver un skill partagé").body(),
    r("skills.shared_remove", "DELETE", "/skills/shared/{skillId}", "Skills", "Retirer un skill partagé").danger(),
    // ----- MCP distants -----
    r("mcp.list", "GET", "/mcp/servers", "MCP distants", "Lister mes serveurs"),
    r("mcp.create", "POST", "/mcp/servers", "MCP distants", "Ajouter un serveur").body(),
    r("mcp.update", "POST", "/mcp/servers/{serverId}", "MCP distants", "Modifier un serveur").body(),
    r("mcp.delete", "DELETE", "/mcp/servers/{serverId}", "MCP distants", "Supprimer un serveur").danger(),
    r("mcp.oauth_start", "POST", "/mcp/servers/{serverId}/oauth/start", "MCP distants", "Démarrer OAuth"),
    r("mcp.oauth_complete", "POST", "/mcp/servers/oauth/complete", "MCP distants", "Terminer OAuth").body(),
    // ----- DeskDock, imports -----
    r("deskdock.get", "GET", "/deskdock", "DeskDock", "Récupérer le PC virtuel"),
    r("deskdock.create", "POST", "/deskdock", "DeskDock", "Créer le PC virtuel"),
    r("deskdock.delete", "DELETE", "/deskdock", "DeskDock", "Supprimer le PC virtuel et ses fichiers").danger(),
    r("deskdock.start", "POST", "/deskdock/start", "DeskDock", "Démarrer"),
    r("deskdock.stop", "POST", "/deskdock/stop", "DeskDock", "Arrêter (fichiers conservés)"),
    r("imports.openai", "POST", "/conversation-imports/openai", "Imports", "Importer une conversation OpenAI").body(),
    // ----- Compte -----
    r("user.update", "PATCH", "/user/@me", "Compte", "Mettre à jour le profil").body(),
    r("user.instructions_get", "GET", "/user/instructions", "Compte", "Instructions personnalisées"),
    r("user.instructions_update", "PATCH", "/user/instructions", "Compte", "Modifier les instructions").body(),
    r("user.notifications_get", "GET", "/user/notification-preferences", "Compte", "Préférences de notifications"),
    r("user.notifications_update", "PATCH", "/user/notification-preferences", "Compte", "Modifier les notifications").body(),
    r("user.usage", "GET", "/user/usage", "Compte", "Usage et limites"),
    r("user.tokenusage", "GET", "/user/tokenusage", "Compte", "Usage de tokens").q(&["tokenUsageDays"]),
    r("user.credits", "GET", "/user/rate-limit-credits", "Compte", "Crédits de rate limit"),
    r("user.credits_activate", "POST", "/user/rate-limit-credits/activate", "Compte", "Activer un power-up (consomme un crédit)").danger(),
    r("user.sessions", "GET", "/user/sessions", "Compte", "Lister mes sessions"),
    r("user.sessions_end_all", "DELETE", "/user/sessions", "Compte", "Terminer toutes les sessions").danger(),
    r("user.session_end", "DELETE", "/user/sessions/{sessionId}", "Compte", "Terminer une session").danger(),
    r("user.pets", "GET", "/user/pets", "Compte", "Valeurs de `pets`"),
    r("user.flags", "GET", "/user/flags", "Compte", "Flags disponibles"),
    r("user.beta_on", "POST", "/user/register-beta", "Compte", "Activer BETA_TESTER"),
    r("user.beta_off", "POST", "/user/unregister-beta", "Compte", "Retirer BETA_TESTER"),
    r("user.dev_on", "POST", "/user/register-active-developer", "Compte", "Activer ACTIVE_DEVELOPER"),
    r("user.dev_off", "POST", "/user/unregister-active-developer", "Compte", "Retirer ACTIVE_DEVELOPER"),
    r("user.invite", "POST", "/user/invite", "Compte", "Inviter par email (5 / 30 jours)").body(),
    r("user.push_key", "GET", "/user/push/vapid-public-key", "Compte", "Clé VAPID"),
    r("user.push_subscribe", "POST", "/user/push/subscriptions", "Compte", "Enregistrer une subscription push").body(),
    r("user.push_unsubscribe", "DELETE", "/user/push/subscriptions", "Compte", "Supprimer la subscription push").body().danger(),
    r("user.memories", "GET", "/user/@me/memories", "Mémoires", "Lister mes mémoires"),
    r("user.memories_delete_all", "DELETE", "/user/@me/memories", "Mémoires", "Supprimer toutes mes mémoires").danger(),
    r("user.memory_get", "GET", "/user/@me/memories/{memoryId}", "Mémoires", "Lire une mémoire"),
    r("user.memory_delete", "DELETE", "/user/@me/memories/{memoryId}", "Mémoires", "Supprimer une mémoire").danger(),
    r("apikey.get", "GET", "/user/@me/api-key", "Clé API", "Consulter ma clé (masquée)"),
    r("apikey.create", "POST", "/user/@me/api-key", "Clé API", "Générer une clé (remplace l'ancienne)").danger(),
    r("apikey.revoke", "DELETE", "/user/@me/api-key", "Clé API", "Révoquer ma clé").danger(),
    r("saved.list", "GET", "/user/@me/saved-messages", "Messages sauvegardés", "Lister").q(&["limit", "cursor"]),
    r("saved.save", "PUT", "/user/@me/saved-messages/{chatId}/{messageId}", "Messages sauvegardés", "Sauvegarder"),
    r("saved.unsave", "DELETE", "/user/@me/saved-messages/{chatId}/{messageId}", "Messages sauvegardés", "Retirer").danger(),
    r("survey.get", "GET", "/user/@me/survey", "Surveys", "Survey actif").q(&["disable_auto_seen", "survey_override"]),
    r("survey.seen", "POST", "/user/@me/survey/{surveyId}/seen", "Surveys", "Marquer comme vu"),
    r("feedback.get", "GET", "/user/@me/feedback-experiences/{experienceId}", "Feedback", "État d'une experience"),
    r("feedback.save", "POST", "/user/@me/feedback-experiences/{experienceId}/responses", "Feedback", "Enregistrer un retour").body(),
    r("websearch.get", "GET", "/user/web-search-domains", "Recherche web", "Filtre de domaines"),
    r("websearch.put", "PUT", "/user/web-search-domains", "Recherche web", "Remplacer le filtre").body(),
    r("websearch.audit", "GET", "/user/web-search-domains/audit-log", "Recherche web", "Historique").q(&["limit", "cursor", "domain"]),
    // ----- Passkeys (WebAuthn : navigateur requis pour les étapes options/finish) -----
    r("passkeys.list", "GET", "/user/passkeys", "Passkeys", "Lister mes passkeys"),
    r("passkeys.reg_options", "POST", "/user/passkeys/registration/options", "Passkeys", "Début d'enregistrement").h(ORIGIN),
    r("passkeys.reg_finish", "POST", "/user/passkeys/registration/finish", "Passkeys", "Fin d'enregistrement").h(ORIGIN).body(),
    r("passkeys.ver_options", "POST", "/user/passkeys/verification/options", "Passkeys", "Début de vérification").h(ORIGIN),
    r("passkeys.ver_finish", "POST", "/user/passkeys/verification/finish", "Passkeys", "Fin de vérification").h(ORIGIN).body(),
    r("passkeys.ver_get", "GET", "/user/passkeys/verification", "Passkeys", "Expiration de ma vérification"),
    r("passkeys.admin_get", "GET", "/user/passkeys/admin-verification", "Passkeys", "Preuve admin de la session"),
    r("passkeys.admin_activity", "POST", "/user/passkeys/admin-verification/activity", "Passkeys", "Activité staff"),
    r("passkeys.rename", "PATCH", "/user/passkeys/{id}", "Passkeys", "Renommer").body(),
    r("passkeys.delete", "DELETE", "/user/passkeys/{id}", "Passkeys", "Supprimer").danger(),
    // ----- Safety Hub, patch notes, public, divers -----
    r("safety.me", "GET", "/safety-hub/@me", "Safety Hub", "Mon Safety Hub"),
    r("safety.appeal", "PUT", "/safety-hub/request-review/{classificationId}", "Safety Hub", "Faire appel").body(),
    r("patchnotes.list", "GET", "/patch-notes", "Patch notes", "Lister"),
    r("patchnotes.get", "GET", "/patch-notes/{year}/{month}/{name}", "Patch notes", "Lire une patch note"),
    r("public.health", "GET", "/public/health", "Public", "État de santé"),
    r("public.alerts", "GET", "/public/alerts", "Public", "Alertes publiques"),
    r("public.data_export", "GET", "/public/data-export/download", "Public", "Télécharger un export (jeton unique)").q(&["token"]).kind(Kind::Binary),
    r("metrics.prometheus", "GET", "/metrics", "Métriques", "Métriques Prometheus (texte)"),
    r("metrics.chat_streams", "GET", "/metrics/chat-streams", "Métriques", "Limite de streams (staff)"),
    r("misc.root", "GET", "/", "Divers", "Page d'accueil de l'API"),
    r("misc.auth_staff", "GET", "/auth/staff", "Divers", "Accès staff"),
    r("misc.ad_challenge", "POST", "/ads/challenges/{challengeId}/complete", "Divers", "Compléter un challenge"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn params(p: &[(&str, &str)]) -> BTreeMap<String, String> {
        p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn ids_and_routes_are_unique() {
        let mut ids = std::collections::BTreeSet::new();
        let mut sigs = std::collections::BTreeSet::new();
 for r in ROUTES {
     assert!(ids.insert(r.id), "id en double : {}", r.id);
     assert!(sigs.insert((r.method, r.path)), "route en double : {} {}", r.method, r.path);
     assert!(r.path.starts_with('/'));
     assert!(["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&r.method));
        }
        assert!(ROUTES.len() >= 125, "catalogue trop petit : {}", ROUTES.len());
    }

    #[test]
    fn risk_rules() {
        for r in ROUTES {
     if r.method == "GET" {
  assert_eq!(r.risk, Risk::Read, "{}", r.id);
     }
     if r.method == "DELETE" && r.id != "chats.unpin" && r.id != "locked.access_close" {
  assert_eq!(r.risk, Risk::Destructive, "DELETE sans confirmation : {}", r.id);
     }
        }
        for id in ["apikey.create", "user.credits_activate", "locked.pin_reset", "deskdock.delete"] {
            assert_eq!(find(id).unwrap().risk, Risk::Destructive, "{id}");
        }
    }

    #[test]
    fn build_path_encodes_and_validates() {
        let r = find("chats.edit_message").unwrap();
        assert_eq!(r.path_params(), vec!["chatId", "messageId"]);
        let p = r.build_path(&params(&[("chatId", "abc"), ("messageId", "d e")])).unwrap();
 assert_eq!(p, "/chats/abc/messages/d%20e");
 assert!(r.build_path(&params(&[("chatId", "abc")])).is_err());
        assert!(r.build_path(&params(&[("chatId", "a"), ("messageId", " ")])).is_err());
        assert!(r.build_path(&params(&[("chatId", "a"), ("messageId", "b"), ("x", "y")])).is_err());
 let evil = r.build_path(&params(&[("chatId", "../../user/@me"), ("messageId", "x?y#z")])).unwrap();
        assert!(!evil[1..].contains("..") || evil.contains("%2F"));
        assert_eq!(evil, "/chats/..%2F..%2Fuser%2F%40me/messages/x%3Fy%23z");
        assert_eq!(encode_segment(".."), "%2E%2E");
 assert_eq!(find("user.memories").unwrap().build_path(&params(&[])).unwrap(), "/user/@me/memories");
    }

    #[test]
    fn extras_are_whitelisted() {
        let r = find("chats.search").unwrap();
        assert!(r.check_extras(&[("query".into(), "a".into())], &[]).is_ok());
        assert!(r.check_extras(&[("evil".into(), "a".into())], &[]).is_err());
        assert!(r.check_extras(&[], &[("authorization".into(), "x".into())]).is_err());
        let l = find("locked.list").unwrap();
        assert!(l.check_extras(&[], &[("x-locked-chats-access".into(), "t".into())]).is_ok());
    }

    #[test]
    fn skill_update_is_post() {
        // L'ancienne commande utilisait PATCH ; la documentation dit POST.
        assert_eq!(find("skills.update").unwrap().method, "POST");
    }

    /// Toute route à méthode unique de la documentation doit être dans le catalogue.
    #[test]
    fn documentation_is_covered() {
 let doc = match std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../API_FranceStudent.md")) {
     Ok(d) => d,
     Err(_) => return,
        };
        let norm = |p: &str| {
     let mut out = String::new();
     let mut depth = 0;
     for c in p.chars() {
         match c {
      '{' => {
          depth += 1;
   out.push_str("{}");
      }
      '}' => depth -= 1,
      _ if depth > 0 => {}
      _ => out.push(c),
  }
     }
     out
        };
 let have: std::collections::BTreeSet<(String, String)> =
     ROUTES.iter().map(|r| (r.method.to_string(), norm(r.path))).collect();
        let mut seen = 0;
        let mut missing = Vec::new();
        for line in doc.lines() {
     let Some(rest) = line.strip_prefix("| `") else { continue };
     let Some((method, rest)) = rest.split_once("` | `") else { continue };
     if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method) {
         continue;
     }
     let Some((path, _)) = rest.split_once('`') else { continue };
     if !path.starts_with('/') || path == "/error" {
         continue;
            }
     seen += 1;
     if !have.contains(&(method.to_string(), norm(path))) {
         missing.push(format!("{method} {path}"));
            }
        }
 assert!(seen > 100, "peu de routes lues dans la doc : {seen}");
        assert!(missing.is_empty(), "routes absentes du catalogue : {missing:?}");
    }
}
