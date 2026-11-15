// Instructions personnalisées du compte (GET / PATCH /user/instructions).
// Commandes Rust : get_user_instructions, update_user_instructions.
import { invoke } from '@tauri-apps/api/core';

export const getUserInstructions = () => invoke('get_user_instructions');

// PATCH partiel : on n'envoie que les champs à changer, ex. { customInstructions: '...' }.
export const updateUserInstructions = (body) => invoke('update_user_instructions', { body });
