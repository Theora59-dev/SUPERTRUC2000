//! Commandes Tauri exposées au frontend.

use crate::cut::ConnectorOptions;
use crate::io_3mf::load_3mf;
use crate::io_obj::load_obj;
use crate::io_stl::load_stl;
use crate::mesh::Mesh;
use crate::workspace::{
    file_stem, ActionResult, Axis, Bed, ExportFormat, TransformOp, Workspace, WorkspaceInfo,
};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::ipc::Response;
use tauri::State;

pub struct WorkspaceState(pub Arc<Mutex<Workspace>>);

impl Default for WorkspaceState {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Workspace::default())))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub files: Vec<String>,
    pub message: String,
}

fn lock(ws: &Arc<Mutex<Workspace>>) -> Result<std::sync::MutexGuard<'_, Workspace>, String> {
    ws.lock().map_err(|_| "État interne verrouillé.".to_string())
}

fn join_err(e: impl std::fmt::Display) -> String {
    format!("Opération interrompue : {e}")
}

fn load_file(path: &Path) -> Result<(Mesh, Vec<String>), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Impossible de lire {} : {e}", path.display()))?;
    match ext.as_str() {
        "stl" => Ok((load_stl(&bytes)?, Vec::new())),
        "obj" => Ok((load_obj(&bytes)?, Vec::new())),
        "3mf" => {
            let loaded = load_3mf(&bytes)?;
            Ok((loaded.mesh, loaded.warnings))
        }
        _ => Err(format!(
            "Format « .{ext} » non pris en charge. Utilisez STL, OBJ ou 3MF."
        )),
    }
}

#[tauri::command]
pub async fn import_model(
    state: State<'_, WorkspaceState>,
    path: String,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(path);
        let (mesh, mut warnings) = load_file(&path)?;
        let triangles = mesh.triangle_count();
        if triangles > 1_500_000 {
            warnings.push(format!(
                "Modèle très lourd ({triangles} triangles) : l'aperçu peut être ralenti."
            ));
        }
        let name = file_stem(&path);
        let mut guard = lock(&ws)?;
        guard.import_mesh(name, mesh)?;
        let info = guard.info();
        let msg = format!(
            "Modèle importé : {} triangles, {:.1} × {:.1} × {:.1} mm.",
            triangles,
            info.parts.first().map(|p| p.size[0]).unwrap_or(0.0),
            info.parts.first().map(|p| p.size[1]).unwrap_or(0.0),
            info.parts.first().map(|p| p.size[2]).unwrap_or(0.0),
        );
        Ok(ActionResult {
            workspace: info,
            message: msg,
            warnings,
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn workspace_info(state: State<'_, WorkspaceState>) -> Result<WorkspaceInfo, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = lock(&ws)?;
        Ok(guard.info())
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn preview_mesh(
    state: State<'_, WorkspaceState>,
    part_id: u64,
) -> Result<Response, String> {
    let ws = state.0.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let guard = lock(&ws)?;
        let part = guard
            .parts
            .iter()
            .find(|p| p.id == part_id)
            .ok_or_else(|| "Pièce introuvable.".to_string())?;
        let mesh = part.mesh.clone();
        drop(guard);
        let mut buf = Vec::with_capacity(8 + mesh.positions.len() * 12 + mesh.indices.len() * 4);
        buf.extend_from_slice(&(mesh.positions.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(mesh.indices.len() as u32).to_le_bytes());
        for p in &mesh.positions {
            buf.extend_from_slice(&p[0].to_le_bytes());
            buf.extend_from_slice(&p[1].to_le_bytes());
            buf.extend_from_slice(&p[2].to_le_bytes());
        }
        for i in &mesh.indices {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        Ok::<Vec<u8>, String>(buf)
    })
    .await
    .map_err(join_err)??;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub async fn cut_part(
    state: State<'_, WorkspaceState>,
    part_id: u64,
    axis: Axis,
    position: f64,
    connectors: ConnectorOptions,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.cut(part_id, axis, position, connectors)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn auto_split(
    state: State<'_, WorkspaceState>,
    ids: Option<Vec<u64>>,
    connectors: bool,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.auto_split(ids, connectors)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn transform_part(
    state: State<'_, WorkspaceState>,
    part_id: u64,
    op: TransformOp,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.transform(part_id, op)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn repair_part(
    state: State<'_, WorkspaceState>,
    part_id: u64,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.repair(part_id)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn hollow_part(
    state: State<'_, WorkspaceState>,
    part_id: u64,
    thickness: f64,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.hollow(part_id, thickness)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn delete_part(
    state: State<'_, WorkspaceState>,
    part_id: u64,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.delete(part_id)?;
        let info = guard.info();
        Ok(ActionResult {
            workspace: info,
            message: "Pièce supprimée.".into(),
            warnings: Vec::new(),
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn duplicate_part(
    state: State<'_, WorkspaceState>,
    part_id: u64,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        let id = guard.duplicate(part_id)?;
        let info = guard.info();
        Ok(ActionResult {
            workspace: info,
            message: format!("Pièce dupliquée (#{id})."),
            warnings: Vec::new(),
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn arrange_parts(
    state: State<'_, WorkspaceState>,
    gap: f64,
) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.arrange(gap)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn set_bed(
    state: State<'_, WorkspaceState>,
    bed: Bed,
) -> Result<WorkspaceInfo, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.set_bed(bed);
        Ok(guard.info())
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn undo(state: State<'_, WorkspaceState>) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        let ok = guard.undo();
        let info = guard.info();
        Ok(ActionResult {
            workspace: info,
            message: if ok {
                "Annulé.".into()
            } else {
                "Rien à annuler.".into()
            },
            warnings: Vec::new(),
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn redo(state: State<'_, WorkspaceState>) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        let ok = guard.redo();
        let info = guard.info();
        Ok(ActionResult {
            workspace: info,
            message: if ok {
                "Rétabli.".into()
            } else {
                "Rien à rétablir.".into()
            },
            warnings: Vec::new(),
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn clear_workspace(state: State<'_, WorkspaceState>) -> Result<ActionResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&ws)?;
        guard.clear();
        let info = guard.info();
        Ok(ActionResult {
            workspace: info,
            message: "Atelier vidé.".into(),
            warnings: Vec::new(),
        })
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn export_parts(
    state: State<'_, WorkspaceState>,
    ids: Option<Vec<u64>>,
    path: String,
    format: ExportFormat,
) -> Result<ExportResult, String> {
    let ws = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = lock(&ws)?;
        let files = guard.export(ids, &path, format)?;
        let message = if files.len() == 1 {
            format!("Export terminé : {}", files[0].display())
        } else {
            format!("{} fichiers exportés dans {}", files.len(), path)
        };
        Ok(ExportResult {
            files: files.iter().map(|p| p.display().to_string()).collect(),
            message,
        })
    })
    .await
    .map_err(join_err)?
}
