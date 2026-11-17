// Fine couche au-dessus des commandes Tauri (voir src-tauri/src/main.rs et ext.rs).
import { invoke, Channel } from '@tauri-apps/api/core';

export const getSettings = () => invoke('get_settings');
export const storeApiToken = (token) => invoke('store_api_token', { token });
export const loginPassword = (email, password) => invoke('login_password', { email, password });
export const logout = () => invoke('logout');

// Seuls les champs fournis (non null) sont modifiés côté Rust.
export const saveSettings = ({
  baseUrl = null,
  apiKey = null,
  lastAgentId = null,
  lastWorkdir = null,
  autoAccept = null,
  eco = null,
} = {}) =>
  invoke('save_settings', {
    baseUrl,
    apiKey: apiKey || null,
    lastAgentId,
    lastWorkdir,
    autoAccept,
    eco,
  });

// Sélecteur de dossier natif ; null si annulé.
export const pickFolder = () => invoke('pick_folder');

export const listAgents = () => invoke('list_agents');
export const getUsage = () => invoke('get_usage');
export const listChats = (limit = 40, cursor = null) => invoke('list_chats', { limit, cursor });
export const getChat = (chatId) => invoke('get_chat', { chatId });
export const deleteChat = (chatId) => invoke('delete_chat', { chatId });
export const renameChat = (chatId, title) => invoke('rename_chat', { chatId, title });
export const pinChat = (chatId, pinned) => invoke('pin_chat', { chatId, pinned: !!pinned });
export const archiveChat = (chatId) => invoke('archive_chat', { chatId });
export const searchChats = (query, limit = 20) => invoke('search_chats', { query, limit });
export const forkEditMessage = (chatId, messageId, content) =>
  invoke('fork_edit_message', { chatId, messageId, content });
export const listSavedMessages = (limit = 50, cursor = null) =>
  invoke('list_saved_messages', { limit, cursor });
export const saveMessage = (chatId, messageId) =>
  invoke('save_message', { chatId, messageId });
export const unsaveMessage = (chatId, messageId) =>
  invoke('unsave_message', { chatId, messageId });
export const listArchivedChats = (limit = 40) =>
  invoke('list_archived_chats', { limit });
export const restoreChat = (chatId) => invoke('restore_chat', { chatId });
export const getNotificationPrefs = () => invoke('get_notification_prefs');
export const updateNotificationPrefs = (prefs) =>
  invoke('update_notification_prefs', { prefs });
export const openExternal = (url) => invoke('open_external', { url });

export async function uploadAttachment(file, kind = null) {
  const ct = file.type || 'application/octet-stream';
  const resolvedKind = kind || (ct.startsWith('image/') ? 'image' : 'file');
  const buf = await file.arrayBuffer();
  return invoke('upload_attachment', {
    kind: resolvedKind,
    filename: file.name || (resolvedKind === 'image' ? 'image.png' : 'fichier.bin'),
    contentType: ct,
    bytes: Array.from(new Uint8Array(buf)),
  });
}

export const getContext = (chatId) => invoke('get_context', { chatId });
export const compactChat = (chatId) => invoke('compact_chat', { chatId });

// Annule UNE génération : `runId` = celui passé à sendMessage. `chatId` peut être null
// (chat pas encore créé côté serveur) : seul le drapeau local est alors levé.
export const cancelChat = (chatId, runId) =>
  invoke('cancel_chat', { chatId: chatId ?? null, runId: runId ?? null });

// Réponse à une demande de confirmation d'outil.
export const resolveTool = (callId, allow) => invoke('resolve_tool', { callId, allow });

// ---- Skills ----
// Skills locaux : [{name, description, source: 'projet'|'utilisateur'|'app', path}]
export const listSkills = (workdir = null) => invoke('list_skills', { workdir });
// Texte brut du SKILL.md d'un skill local.
export const skillRead = (name, workdir = null) => invoke('skill_read', { name, workdir });
// Remplace le SKILL.md d'un skill local existant.
export const skillWrite = (name, text, workdir = null) =>
  invoke('skill_write', { name, workdir, text });
// Crée un skill dans le dossier de l'app (<config>/skills/<nom>/SKILL.md).
export const skillCreate = (name, description, body, workdir = null) =>
  invoke('skill_create', { name, description, body, workdir });
// Supprime le dossier du skill (définitif).
export const skillDelete = (name, workdir = null) => invoke('skill_delete', { name, workdir });
// Skills du compte (`GET /skills`, confirmé : la liste s'affiche).
export const accountSkills = () => invoke('account_skills');
// Modifier / retirer un skill du compte : `PATCH` / `DELETE /skills/{id}` SUPPOSÉS.
export const accountSkillUpdate = (id, body) => invoke('account_skill_update', { id, body });
export const accountSkillDelete = (id) => invoke('account_skill_delete', { id });

// ---- MCP local ----
// {servers:[{name,command,args,connected,tools:[{name,exposedName,description,readOnly,trusted}]}], error, customRejected}
export const mcpStatus = () => invoke('mcp_status');
export const mcpGetConfig = () => invoke('mcp_get_config'); // texte JSON brut de mcp.json
export const mcpSaveConfig = (text) => invoke('mcp_save_config', { text });
export const mcpConnect = (name) => invoke('mcp_connect', { name }); // -> nombre d'outils
export const mcpDisconnect = (name) => invoke('mcp_disconnect', { name });
// Appel manuel d'un outil, lancé par l'utilisateur (jamais par le modèle) -> {text, isError}.
export const mcpCall = (server, tool, args = {}) => invoke('mcp_call', { server, tool, args });
// Configuration d'un serveur : {command, args, env, cwd}.
export const mcpGetServer = (name) => invoke('mcp_get_server', { name });
// Ajout (oldName = null) ou modification (oldName = nom actuel, peut changer de nom).
// Le serveur est déconnecté par l'opération.
export const mcpUpsert = (oldName, name, config) =>
  invoke('mcp_upsert', { oldName: oldName ?? null, name, config });
export const mcpRemove = (name) => invoke('mcp_remove', { name });
// Importe du JSON {"mcpServers": {...}} ; renvoie les noms ajoutés.
export const mcpImport = (text) => invoke('mcp_import', { text });
// « Toujours autoriser » un outil MCP pour cette session.
export const trustTool = (name, on) => invoke('trust_tool', { name, on });

// Envoie un message. `workdir` renseigné => mode Code (outils de fichiers).
// `eco` : true/false force le mode économie ; absent => réglage enregistré.
// `runId` : identifiant de cette génération (pour l'annuler seule). Plusieurs générations
// peuvent tourner en même temps (une par chat) : chaque appel a son propre canal.
// `onEvent` reçoit {type, ...} avec type ∈
//   opened{chat_id} | title{chat_id,title} | delta{text} | reasoning{text}
//   | tool_call{call_id,name,origin('local'),args,needs_confirm}
//   | tool_result{call_id,output,is_error,denied}
//   | notice{text}
//   | done{chat_id,message_id,prompt_tokens,output_tokens,total_prompt_tokens,total_output_tokens,api_calls,eco}
//   | failed{code,message,retryable} | cancelled
//   | resumed{run_id} | awaiting_tools{run_id,calls} | run_interrupted{run_id,reason}
// Le texte envoyé est exactement `content` : aucun contexte n'est ajouté.
// La promesse se résout quand tout est terminé (outils compris).
export function sendMessage(
  { chatId, agentId, content, workdir, autoAccept, eco, runId, images, files, enableWebSearch },
  onEvent
) {
  const channel = new Channel();
  channel.onmessage = onEvent;
  return invoke('send_message', {
    chatId: chatId ?? null,
    agentId,
    content,
    workdir: workdir ?? null,
    autoAccept: !!autoAccept,
    eco: eco ?? null,
    runId: runId ?? null,
    images: images?.length ? images : null,
    files: files?.length ? files : null,
    enableWebSearch: !!enableWebSearch,
    onEvent: channel,
  });
}

// Runs interrompus (redémarrage, coupure réseau, appels en attente).
export const listRuns = () => invoke('list_runs');
export const abandonRun = (runId) => invoke('abandon_run', { runId });
export function resumeRun(runId, onEvent) {
  const channel = new Channel();
  channel.onmessage = onEvent;
  return invoke('resume_run', { runId, onEvent: channel });
}

// Phase 2 : synchro des instructions (bloc FS Code) et Phase 4 : explorateur d'API.
export const instructionsStatus = () => invoke('instructions_status');
export const instructionsSync = () => invoke('instructions_sync');
export const instructionsRemove = () => invoke('instructions_remove');
export const instructionsSetEnabled = (enabled) => invoke('instructions_set_enabled', { enabled: !!enabled });
export const apiRoutes = () => invoke('api_routes');
export const apiCall = ({ id, pathParams, query, headers, body, confirmed }) =>
  invoke('api_call', { id, pathParams, query, headers, body, confirmed: !!confirmed });
