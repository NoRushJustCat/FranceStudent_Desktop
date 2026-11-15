export const THEME_ALIASES = {
  'fs-light': 'clair',
  'fs-dark': 'sombre',
  'fs-gradien': 'gradien',
  'fs-aurora': 'aurora',
};

export function normalizeTheme(id) {
  return THEME_ALIASES[id] || id || 'sombre';
}

export const THEMES = [
  { id: 'clair', label: 'Clair', light: true, bg: '#ffffff', side: '#fafafa', card: '#f4f4f5', cardBorder: '#e4e4e7', primary: '#0139b7', text: '#09090b' },
  { id: 'gris', label: 'Gris', light: false, bg: '#1e1e1e', side: '#171717', card: '#262626', cardBorder: '#383838', primary: '#67a6e4', text: '#f4f4f5' },
  { id: 'sombre', label: 'Sombre', light: false, bg: '#09090b', side: '#18181b', card: '#27272a', cardBorder: '#3f3f46', primary: '#2662d9', text: '#fafafa' },
  { id: 'gradien', label: 'Dégradé', light: false, bg: '#19175f', side: '#111044', card: '#24237a', cardBorder: '#37369e', primary: '#5865f2', text: '#f8fafc' },
  { id: 'graphite', label: 'Graphite', light: false, bg: '#2d2d2b', side: '#242422', card: '#3a3a37', cardBorder: '#4e4e4a', primary: '#cc7d5e', text: '#f5f5f4' },
  { id: 'aurora', label: 'Aurore', light: false, bg: '#0d2c33', side: '#05191f', card: '#133c46', cardBorder: '#1d5361', primary: '#6ee7c8', text: '#ecfeff' },
  { id: 'synthwave', label: 'Néon', light: false, bg: '#24104a', side: '#180832', card: '#311663', cardBorder: '#4c2594', primary: '#ff4fd8', text: '#fdf4ff' },
  { id: 'lagoon', label: 'Lagon', light: false, bg: '#0a2940', side: '#061724', card: '#103854', cardBorder: '#1b537a', primary: '#4fd1c5', text: '#f0fdfa' },
  { id: 'ember', label: 'Braise', light: false, bg: '#26130f', side: '#1c0d0a', card: '#361b15', cardBorder: '#522920', primary: '#fb923c', text: '#fff7ed' },
  { id: 'matcha', label: 'Matcha', light: false, bg: '#1f2d22', side: '#111b13', card: '#283b2c', cardBorder: '#3b5741', primary: '#86efac', text: '#f0fdf4' },
  { id: 'midnight', label: 'Minuit', light: false, bg: '#070b17', side: '#040810', card: '#10172a', cardBorder: '#1e293b', primary: '#93c5fd', text: '#f8fafc' },
  { id: 'rose', label: 'Rose', light: false, bg: '#3a1d31', side: '#2a1122', card: '#4c2640', cardBorder: '#6e375d', primary: '#f9a8d4', text: '#fdf2f8' },
  { id: 'citrus', label: 'Agrumes', light: false, bg: '#282511', side: '#1c1909', card: '#383418', cardBorder: '#544e24', primary: '#facc15', text: '#fefce8' },
  { id: 'ocean', label: 'Océan', light: false, bg: '#082538', side: '#05151f', card: '#0e334d', cardBorder: '#184e73', primary: '#38bdf8', text: '#f0f9ff' },
  { id: 'dusk', label: 'Crépuscule', light: false, bg: '#241b40', side: '#150f25', card: '#312557', cardBorder: '#47367d', primary: '#a78bfa', text: '#f5f3ff' },
  { id: 'obsidian', label: 'Obsidienne', light: false, bg: '#05070c', side: '#05080e', card: '#0d1322', cardBorder: '#1c2642', primary: '#c7d2fe', text: '#f8fafc' },
  { id: 'amethyst', label: 'Améthyste', light: false, bg: '#1b1230', side: '#140c23', card: '#271a45', cardBorder: '#3c2869', primary: '#c084fc', text: '#faf5ff' },
  { id: 'spruce', label: 'Sapin', light: false, bg: '#0d1f1a', side: '#08120f', card: '#143028', cardBorder: '#204a3e', primary: '#6ee7b7', text: '#ecfdf5' },
  { id: 'ruby', label: 'Rubis', light: false, bg: '#2a0d14', side: '#19080d', card: '#3b121c', cardBorder: '#591c2b', primary: '#fb7185', text: '#fff1f2' },
  { id: 'cobalt', label: 'Cobalt', light: false, bg: '#081a33', side: '#050f1e', card: '#0e264a', cardBorder: '#193c70', primary: '#60a5fa', text: '#eff6ff' },
];
