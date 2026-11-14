// Fine couche au-dessus des commandes Tauri (voir src-tauri/src/main.rs et ext.rs).
import { invoke, Channel } from '@tauri-apps/api/core';

export const getSettings = () => invoke('get_settings');

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
// La promesse se résout quand tout est terminé (outils compris).
export function sendMessage({ chatId, agentId, content, workdir, autoAccept, eco, runId }, onEvent) {
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
    onEvent: channel,
  });
}
