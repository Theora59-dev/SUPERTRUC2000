import { invoke } from "@tauri-apps/api/core";

export type Axis = "x" | "y" | "z";
export type ExportFormat = "stl" | "obj" | "3mf";

export interface Bed {
  x: number;
  y: number;
  z: number;
  margin: number;
}

export interface PartMeta {
  id: number;
  name: string;
  triangles: number;
  vertices: number;
  volume: number;
  area: number;
  min: [number, number, number];
  max: [number, number, number];
  size: [number, number, number];
  center: [number, number, number];
  revision: number;
}

export interface WorkspaceInfo {
  parts: PartMeta[];
  bed: Bed;
  undoDepth: number;
  redoDepth: number;
  totalTriangles: number;
  totalVolume: number;
}

export interface ActionResult {
  workspace: WorkspaceInfo;
  message: string;
  warnings: string[];
}

export interface ExportResult {
  files: string[];
  message: string;
}

export interface ConnectorOptions {
  enabled: boolean;
  auto: boolean;
  count: number;
  radius: number;
  depth: number;
  clearance: number;
}

export type TransformOp =
  | { kind: "translate"; x: number; y: number; z: number }
  | { kind: "rotate"; axis: Axis; degrees: number }
  | { kind: "scale"; factor: number }
  | { kind: "size"; height: number }
  | { kind: "fitBed" }
  | { kind: "dropToBed" }
  | { kind: "center" }
  | { kind: "mirror"; axis: Axis };

export const defaultBed: Bed = { x: 220, y: 220, z: 250, margin: 5 };

export const defaultConnectors: ConnectorOptions = {
  enabled: true,
  auto: true,
  count: 4,
  radius: 3,
  depth: 8,
  clearance: 0.2,
};

export const importModel = (path: string) =>
  invoke<ActionResult>("import_model", { path });

export const workspaceInfo = () => invoke<WorkspaceInfo>("workspace_info");

export const previewMesh = (partId: number) =>
  invoke<ArrayBuffer>("preview_mesh", { partId });

export const cutPart = (
  partId: number,
  axis: Axis,
  position: number,
  connectors: ConnectorOptions,
) => invoke<ActionResult>("cut_part", { partId, axis, position, connectors });

export const autoSplit = (ids: number[] | null, connectors: boolean) =>
  invoke<ActionResult>("auto_split", { ids, connectors });

export const transformPart = (partId: number, op: TransformOp) =>
  invoke<ActionResult>("transform_part", { partId, op });

export const repairPart = (partId: number) =>
  invoke<ActionResult>("repair_part", { partId });

export const hollowPart = (partId: number, thickness: number) =>
  invoke<ActionResult>("hollow_part", { partId, thickness });

export const deletePart = (partId: number) =>
  invoke<ActionResult>("delete_part", { partId });

export const duplicatePart = (partId: number) =>
  invoke<ActionResult>("duplicate_part", { partId });

export const arrangeParts = (gap: number) =>
  invoke<ActionResult>("arrange_parts", { gap });

export const setBed = (bed: Bed) => invoke<WorkspaceInfo>("set_bed", { bed });

export const undo = () => invoke<ActionResult>("undo");
export const redo = () => invoke<ActionResult>("redo");
export const clearWorkspace = () => invoke<ActionResult>("clear_workspace");

export const exportParts = (
  ids: number[] | null,
  path: string,
  format: ExportFormat,
) => invoke<ExportResult>("export_parts", { ids, path, format });
