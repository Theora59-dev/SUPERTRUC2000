//! SUPERTRUC 2000 — atelier de découpe et de préparation de grands modèles 3D.

pub mod commands;
pub mod cut;
pub mod io_3mf;
pub mod io_obj;
pub mod io_stl;
pub mod mesh;
pub mod workspace;

use commands::WorkspaceState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(WorkspaceState::default())
        .invoke_handler(tauri::generate_handler![
            commands::import_model,
            commands::workspace_info,
            commands::preview_mesh,
            commands::cut_part,
            commands::auto_split,
            commands::transform_part,
            commands::repair_part,
            commands::hollow_part,
            commands::delete_part,
            commands::duplicate_part,
            commands::arrange_parts,
            commands::set_bed,
            commands::undo,
            commands::redo,
            commands::clear_workspace,
            commands::export_parts,
        ])
        .run(tauri::generate_context!())
        .expect("Erreur au lancement de SUPERTRUC 2000");
}
