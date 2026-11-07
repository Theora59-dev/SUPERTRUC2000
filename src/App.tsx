import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  arrangeParts,
  autoSplit,
  clearWorkspace,
  cutPart,
  defaultBed,
  defaultConnectors,
  deletePart,
  duplicatePart,
  exportParts,
  hollowPart,
  importModel,
  redo,
  repairPart,
  setBed as updateBed,
  transformPart,
  undo,
  workspaceInfo,
  type ActionResult,
  type Axis,
  type Bed,
  type ConnectorOptions,
  type ExportFormat,
  type ExportResult,
  type PartMeta,
  type TransformOp,
  type WorkspaceInfo,
} from "./api";
import {
  Viewport,
  partColorAt,
  type PlanePreview,
  type ViewCommand,
  type ViewKind,
} from "./Viewport";
import {
  BrandMark,
  IconAlert,
  IconAuto,
  IconCheck,
  IconClose,
  IconCopy,
  IconExport,
  IconEye,
  IconEyeOff,
  IconFit,
  IconFolder,
  IconImport,
  IconInfo,
  IconRefresh,
  IconScissors,
  IconSliders,
  IconTrash,
  IconUndo,
  IconRedo,
} from "./Icons";

type TabId = "cut" | "auto" | "edit" | "export";
type ToastKind = "success" | "warn" | "error" | "info";
type Scope = "all" | "selected";

interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  details?: string[];
}

const EMPTY_WORKSPACE: WorkspaceInfo = {
  parts: [],
  bed: defaultBed,
  undoDepth: 0,
  redoDepth: 0,
  totalTriangles: 0,
  totalVolume: 0,
};

const AXES: Axis[] = ["x", "y", "z"];
const AXIS_INDEX: Record<Axis, number> = { x: 0, y: 1, z: 2 };
const MAX_PIECES = 256;

const number = new Intl.NumberFormat("fr-FR");
const fmtInt = (value: number) => number.format(Math.round(value));
const fmt1 = (value: number) =>
  value.toLocaleString("fr-FR", { maximumFractionDigits: 1 });
const fmt2 = (value: number) =>
  value.toLocaleString("fr-FR", { maximumFractionDigits: 2 });
const fmt0 = (value: number) => number.format(Math.round(value));
const fmtSize = (size: number[]) => size.map((v) => fmt1(v)).join(" × ");
const fmtVolume = (mm3: number) =>
  `${(mm3 / 1000).toLocaleString("fr-FR", { maximumFractionDigits: 1 })} cm³`;

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function parseNumber(value: string, fallback = 0) {
  const parsed = Number.parseFloat(value.replace(",", "."));
  return Number.isFinite(parsed) ? parsed : fallback;
}

function ensureExtension(path: string, extension: string) {
  return path.toLowerCase().endsWith(`.${extension}`) ? path : `${path}.${extension}`;
}

function pieceCount(part: PartMeta, bed: Bed) {
  const usable = [
    Math.max(bed.x - 2 * bed.margin, 10),
    Math.max(bed.y - 2 * bed.margin, 10),
    Math.max(bed.z - 2 * bed.margin, 10),
  ];
  return [0, 1, 2].reduce(
    (total, i) =>
      total * Math.max(1, Math.ceil(part.size[i] / usable[i] - 1e-9)),
    1,
  );
}

const EXPORT_HINTS: Record<ExportFormat, string> = {
  stl: "Un fichier STL par pièce. Pour plusieurs pièces, choisissez un dossier de destination.",
  obj: "Un seul fichier OBJ avec toutes les pièces, réparties automatiquement sur le plateau.",
  "3mf": "Un seul fichier 3MF avec toutes les pièces, réparties automatiquement sur le plateau.",
};

const TAB_META: Array<{ id: TabId; label: string; icon: ReactNode }> = [
  { id: "cut", label: "Découpe", icon: <IconScissors size={17} /> },
  { id: "auto", label: "Auto", icon: <IconAuto size={17} /> },
  { id: "edit", label: "Édition", icon: <IconSliders size={17} /> },
  { id: "export", label: "Export", icon: <IconExport size={17} /> },
];

interface NumberFieldProps {
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  disabled?: boolean;
}

function NumberField({
  label,
  value,
  onChange,
  min,
  max,
  step = 1,
  unit = "mm",
  disabled,
}: NumberFieldProps) {
  return (
    <div className="field">
      <div className="field-label">
        <span>{label}</span>
      </div>
      <div className="input-with-unit">
        <input
          className="input"
          type="number"
          value={Number.isFinite(value) ? value : 0}
          min={min}
          max={max}
          step={step}
          disabled={disabled}
          onChange={(event) => {
            const parsed = Number.parseFloat(event.target.value);
            onChange(Number.isFinite(parsed) ? parsed : 0);
          }}
        />
        {unit ? <span className="unit">{unit}</span> : null}
      </div>
    </div>
  );
}

function ToastGlyph({ kind }: { kind: ToastKind }) {
  if (kind === "success") return <IconCheck size={16} />;
  if (kind === "info") return <IconInfo size={16} />;
  return <IconAlert size={16} />;
}

function Switch({
  on,
  onToggle,
  disabled,
  label,
}: {
  on: boolean;
  onToggle: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      className={`switch ${on ? "on" : ""}`}
      aria-pressed={on}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onToggle}
    />
  );
}

export default function App() {
  const [ws, setWs] = useState<WorkspaceInfo>(EMPTY_WORKSPACE);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [hidden, setHidden] = useState<Set<number>>(() => new Set());
  const [tab, setTab] = useState<TabId>("cut");
  const [busy, setBusy] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [status, setStatus] = useState(
    "Prêt — importez un modèle STL, OBJ ou 3MF pour commencer.",
  );
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(1);
  const commandNonce = useRef(0);
  const [command, setCommand] = useState<ViewCommand>({ kind: "fit", nonce: 0 });
  const [exploded, setExploded] = useState(false);

  // Découpe manuelle
  const [axis, setAxis] = useState<Axis>("z");
  const [cutPosition, setCutPosition] = useState(0);
  const cutKey = useRef("");
  const [connectors, setConnectors] = useState<ConnectorOptions>({
    ...defaultConnectors,
  });

  // Découpe automatique
  const [bedDraft, setBedDraft] = useState<Bed>(defaultBed);
  const [autoScope, setAutoScope] = useState<Scope>("all");
  const [autoConnectors, setAutoConnectors] = useState(true);
  const [arrangeGap, setArrangeGap] = useState(4);

  // Édition
  const [tx, setTx] = useState(0);
  const [ty, setTy] = useState(0);
  const [tz, setTz] = useState(0);
  const [rotationAxis, setRotationAxis] = useState<Axis>("z");
  const [rotationDegrees, setRotationDegrees] = useState(90);
  const [scalePercent, setScalePercent] = useState(100);
  const [targetHeight, setTargetHeight] = useState(0);
  const [wallThickness, setWallThickness] = useState(2);

  // Export
  const [format, setFormat] = useState<ExportFormat>("stl");
  const [exportScope, setExportScope] = useState<Scope>("all");
  const [exportPath, setExportPath] = useState("");
  const [lastExport, setLastExport] = useState<ExportResult | null>(null);

  const parts = ws.parts;
  const bed = ws.bed;
  const selectedPart = useMemo(
    () => parts.find((part) => part.id === selectedId) ?? null,
    [parts, selectedId],
  );
  const selectedIndex = useMemo(
    () => parts.findIndex((part) => part.id === selectedId),
    [parts, selectedId],
  );

  const bedDirty =
    Math.abs(bedDraft.x - bed.x) > 0.001 ||
    Math.abs(bedDraft.y - bed.y) > 0.001 ||
    Math.abs(bedDraft.z - bed.z) > 0.001 ||
    Math.abs(bedDraft.margin - bed.margin) > 0.001;

  const exportNeedsDirectory = format === "stl" && exportScope === "all" && parts.length > 1;
  const exportCount =
    exportScope === "selected" ? (selectedPart ? 1 : 0) : parts.length;

  const autoTargets = useMemo(
    () =>
      autoScope === "selected" && selectedPart
        ? [selectedPart]
        : parts,
    [autoScope, selectedPart, parts],
  );
  const autoEstimate = useMemo(
    () => autoTargets.reduce((total, part) => total + pieceCount(part, bed), 0),
    [autoTargets, bed],
  );

  const pushToast = useCallback(
    (kind: ToastKind, title: string, details?: string[]) => {
      const id = toastId.current++;
      setToasts((current) => [...current.slice(-4), { id, kind, title, details }]);
      window.setTimeout(
        () => {
          setToasts((current) => current.filter((toast) => toast.id !== id));
        },
        kind === "error" ? 9000 : 5600,
      );
    },
    [],
  );

  const fail = useCallback(
    (error: unknown) => {
      const message =
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "Erreur inconnue.";
      setStatus(message);
      pushToast("error", "Opération impossible", [message]);
    },
    [pushToast],
  );

  const run = useCallback(
    async <T,>(label: string, action: () => Promise<T>, done: (value: T) => void) => {
      setBusy(label);
      try {
        const value = await action();
        done(value);
      } catch (error) {
        fail(error);
      } finally {
        setBusy(null);
      }
    },
    [fail],
  );

  const applyResult = useCallback(
    (result: ActionResult) => {
      setWs(result.workspace);
      setStatus(result.message);
      if (result.message) {
        pushToast(
          result.warnings.length > 0 ? "warn" : "success",
          result.message,
          result.warnings.length > 0 ? result.warnings : undefined,
        );
      }
    },
    [pushToast],
  );

  const requestView = useCallback((kind: ViewKind) => {
    commandNonce.current += 1;
    setCommand({ kind, nonce: commandNonce.current });
  }, []);

  // Synchronise l'état avec le backend au démarrage.
  useEffect(() => {
    let cancelled = false;
    workspaceInfo()
      .then((info) => {
        if (!cancelled) setWs(info);
      })
      .catch(() => {
        // En aperçu navigateur, le backend Tauri n'est pas disponible.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Garde une sélection et un masquage cohérents.
  useEffect(() => {
    const ids = new Set(parts.map((part) => part.id));
    setSelectedId((current) => {
      if (current !== null && ids.has(current)) return current;
      return parts.length > 0 ? parts[0].id : null;
    });
    setHidden((current) => {
      let changed = false;
      const next = new Set<number>();
      for (const id of current) {
        if (ids.has(id)) next.add(id);
        else changed = true;
      }
      return changed ? next : current;
    });
  }, [parts]);

  // Le brouillon du plateau suit l'état réel.
  useEffect(() => {
    setBedDraft(bed);
  }, [bed]);

  // Position de coupe par défaut au milieu de la pièce / de l'axe.
  useEffect(() => {
    if (!selectedPart) return;
    const key = `${selectedPart.id}:${selectedPart.revision}:${axis}`;
    if (cutKey.current === key) return;
    cutKey.current = key;
    const index = AXIS_INDEX[axis];
    setCutPosition((selectedPart.min[index] + selectedPart.max[index]) / 2);
  }, [selectedPart, axis]);

  // Hauteur cible initialisée sur la pièce sélectionnée.
  useEffect(() => {
    if (!selectedPart) return;
    setTargetHeight(
      Math.round(selectedPart.size[2] * 10) / 10,
    );
  }, [selectedPart?.id, selectedPart?.revision]);

  // Le chemin d'export dépend du format et de la portée.
  useEffect(() => {
    setExportPath("");
    setLastExport(null);
  }, [format, exportScope]);

  const importPath = useCallback(
    async (path: string) => {
      await run("Importation du modèle…", () => importModel(path), (result) => {
        applyResult(result);
        setTab("cut");
        requestView("fit");
      });
    },
    [run, applyResult, requestView],
  );

  const importPathRef = useRef(importPath);
  useEffect(() => {
    importPathRef.current = importPath;
  }, [importPath]);

  const handleImport = useCallback(async () => {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Importer un modèle 3D",
        filters: [
          { name: "Modèles 3D", extensions: ["stl", "obj", "3mf"] },
        ],
      });
      if (typeof picked === "string" && picked) {
        await importPath(picked);
      }
    } catch (error) {
      fail(error);
    }
  }, [importPath, fail]);

  // Glisser-déposer natif Tauri.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let webview;
    try {
      webview = getCurrentWebview();
    } catch {
      // Hors Tauri (aperçu navigateur), ignorer.
      return;
    }
    webview
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter") {
          setDragging(true);
          return;
        }
        if (payload.type === "over") return;
        if (payload.type === "drop") {
          setDragging(false);
          const path = payload.paths.find((candidate) =>
            /\.(stl|obj|3mf)$/i.test(candidate),
          );
          if (path) {
            void importPathRef.current(path);
          } else if (payload.paths.length > 0) {
            pushToast("warn", "Fichier non pris en charge", [
              "Formats acceptés : STL, OBJ ou 3MF.",
            ]);
          }
          return;
        }
        setDragging(false);
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      .catch(() => {
        // Hors Tauri (aperçu navigateur), ignorer.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [pushToast]);

  const doUndo = useCallback(() => {
    void run("Annulation…", undo, applyResult);
  }, [run, applyResult]);

  const doRedo = useCallback(() => {
    void run("Rétablissement…", redo, applyResult);
  }, [run, applyResult]);

  const doClear = useCallback(() => {
    if (parts.length === 0) return;
    void run("Nettoyage de l'atelier…", clearWorkspace, applyResult);
  }, [parts.length, run, applyResult]);

  const doDelete = useCallback(
    (id: number) => {
      void run("Suppression de la pièce…", () => deletePart(id), applyResult);
    },
    [run, applyResult],
  );

  const doDuplicate = useCallback(
    (id: number) => {
      const before = new Set(parts.map((part) => part.id));
      void run("Duplication de la pièce…", () => duplicatePart(id), (result) => {
        applyResult(result);
        const added = result.workspace.parts.find((part) => !before.has(part.id));
        if (added) setSelectedId(added.id);
      });
    },
    [parts, run, applyResult],
  );

  const doRepair = useCallback(() => {
    if (selectedId === null) return;
    void run("Réparation du maillage…", () => repairPart(selectedId), applyResult);
  }, [selectedId, run, applyResult]);

  const doHollow = useCallback(() => {
    if (!selectedPart) {
      pushToast("warn", "Aucune pièce sélectionnée", [
        "Sélectionnez la pièce à évider.",
      ]);
      return;
    }
    if (!(wallThickness > 0)) {
      pushToast("warn", "Épaisseur invalide", [
        "Indiquez une épaisseur de paroi supérieure à 0 mm.",
      ]);
      return;
    }
    void run(
      "Évidement de la pièce…",
      () => hollowPart(selectedPart.id, wallThickness),
      applyResult,
    );
  }, [selectedPart, wallThickness, pushToast, run, applyResult]);

  const doArrange = useCallback(() => {
    if (parts.length === 0) return;
    void run("Disposition sur le plateau…", () => arrangeParts(arrangeGap), (result) => {
      applyResult(result);
      requestView("fit");
    });
  }, [parts.length, arrangeGap, run, applyResult, requestView]);

  const doAutoSplit = useCallback(() => {
    if (parts.length === 0) return;
    const ids =
      autoScope === "selected" && selectedId !== null ? [selectedId] : null;
    void run("Découpe automatique en cours…", () => autoSplit(ids, autoConnectors), (result) => {
      applyResult(result);
      requestView("fit");
    });
  }, [
    parts.length,
    autoScope,
    selectedId,
    autoConnectors,
    run,
    applyResult,
    requestView,
  ]);

  const doCut = useCallback(() => {
    if (!selectedPart) return;
    const index = AXIS_INDEX[axis];
    const lo = selectedPart.min[index];
    const hi = selectedPart.max[index];
    if (cutPosition <= lo + 1e-6 || cutPosition >= hi - 1e-6) {
      pushToast("warn", "Plan hors de la pièce", [
        "Placez le plan de coupe à l'intérieur de la pièce.",
      ]);
      return;
    }
    void run(
      "Découpe de la pièce…",
      () => cutPart(selectedPart.id, axis, cutPosition, connectors),
      (result) => {
        applyResult(result);
        requestView("fit");
      },
    );
  }, [
    selectedPart,
    axis,
    cutPosition,
    connectors,
    pushToast,
    run,
    applyResult,
    requestView,
  ]);

  const applyBed = useCallback(() => {
    const next: Bed = {
      x: clamp(bedDraft.x || 220, 20, 5000),
      y: clamp(bedDraft.y || 220, 20, 5000),
      z: clamp(bedDraft.z || 250, 20, 5000),
      margin: clamp(bedDraft.margin, 0, 50),
    };
    void run("Mise à jour du plateau…", () => updateBed(next), (info) => {
      setWs(info);
      const message = `Plateau ${fmt0(next.x)} × ${fmt0(next.y)} × ${fmt0(next.z)} mm.`;
      setStatus(message);
      pushToast("success", "Volume d'impression mis à jour.", [
        `${message} Marge ${fmt1(next.margin)} mm.`,
      ]);
    });
  }, [bedDraft, run, pushToast]);

  const applyTransform = useCallback(
    (op: TransformOp, label: string) => {
      if (selectedId === null) return;
      void run(label, () => transformPart(selectedId, op), applyResult);
    },
    [selectedId, run, applyResult],
  );

  const doTranslate = () => {
    if (tx === 0 && ty === 0 && tz === 0) {
      pushToast("warn", "Déplacement nul", ["Saisissez au moins une valeur."]);
      return;
    }
    applyTransform({ kind: "translate", x: tx, y: ty, z: tz }, "Déplacement de la pièce…");
  };

  const doRotate = () => {
    if (rotationDegrees === 0) {
      pushToast("warn", "Rotation nulle", ["Saisissez un angle différent de zéro."]);
      return;
    }
    applyTransform(
      { kind: "rotate", axis: rotationAxis, degrees: rotationDegrees },
      "Rotation de la pièce…",
    );
  };

  const doScale = () => {
    if (!(scalePercent > 0)) {
      pushToast("warn", "Échelle invalide", ["Le pourcentage doit être supérieur à 0."]);
      return;
    }
    applyTransform(
      { kind: "scale", factor: scalePercent / 100 },
      "Mise à l'échelle…",
    );
  };

  const doSetHeight = () => {
    if (!(targetHeight > 0)) {
      pushToast("warn", "Hauteur invalide", ["Indiquez une hauteur cible en millimètres."]);
      return;
    }
    applyTransform({ kind: "size", height: targetHeight }, "Redimensionnement…");
  };

  const exportBaseName = useMemo(() => {
    const source =
      exportScope === "selected" && selectedPart
        ? selectedPart.name
        : parts[0]?.name ?? "modele";
    return (
      source
        .split(" #")[0]
        .trim()
        .replace(/[^A-Za-z0-9_\- ]+/g, "_")
        .replace(/\s+/g, "_") || "modele"
    );
  }, [exportScope, selectedPart, parts]);

  const chooseExportPath = useCallback(async (): Promise<string | null> => {
    try {
      if (exportNeedsDirectory) {
        const picked = await open({
          directory: true,
          multiple: false,
          title: "Choisir le dossier d'export STL",
        });
        if (typeof picked === "string" && picked) {
          setExportPath(picked);
          return picked;
        }
        return null;
      }
      const filters =
        format === "obj"
          ? [{ name: "Modèle OBJ", extensions: ["obj"] }]
          : format === "3mf"
            ? [{ name: "Modèle 3MF", extensions: ["3mf"] }]
            : [{ name: "Modèle STL", extensions: ["stl"] }];
      const picked = await save({
        title: "Exporter le modèle",
        defaultPath: `${exportBaseName}.${format}`,
        filters,
      });
      if (typeof picked === "string" && picked) {
        const withExtension = ensureExtension(picked, format);
        setExportPath(withExtension);
        return withExtension;
      }
      return null;
    } catch (error) {
      fail(error);
      return null;
    }
  }, [exportNeedsDirectory, format, exportBaseName, fail]);

  const doExport = useCallback(async () => {
    if (parts.length === 0) return;
    if (exportScope === "selected" && !selectedPart) {
      pushToast("warn", "Aucune pièce sélectionnée", [
        "Choisissez une pièce dans la liste ou dans la vue 3D.",
      ]);
      return;
    }
    let path = exportPath;
    if (!path) {
      const picked = await chooseExportPath();
      if (!picked) return;
      path = picked;
    }
    const finalPath = exportNeedsDirectory ? path : ensureExtension(path, format);
    const ids =
      exportScope === "selected" && selectedPart ? [selectedPart.id] : null;
    void run("Export en cours…", () => exportParts(ids, finalPath, format), (result) => {
      setLastExport(result);
      setStatus(result.message);
      pushToast("success", "Export terminé.", [result.message]);
    });
  }, [
    parts.length,
    exportScope,
    selectedPart,
    exportPath,
    chooseExportPath,
    exportNeedsDirectory,
    format,
    run,
    pushToast,
  ]);

  // Raccourcis clavier.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable)
      ) {
        return;
      }
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (mod && key === "o") {
        event.preventDefault();
        void handleImport();
      } else if (mod && key === "z") {
        event.preventDefault();
        if (event.shiftKey) doRedo();
        else doUndo();
      } else if (mod && key === "y") {
        event.preventDefault();
        doRedo();
      } else if (mod && key === "d") {
        event.preventDefault();
        if (selectedId !== null) doDuplicate(selectedId);
      } else if (event.key === "Delete" && selectedId !== null) {
        event.preventDefault();
        doDelete(selectedId);
      } else if (!mod && key === "f") {
        event.preventDefault();
        requestView("fit");
      } else if (!mod && key === "e") {
        event.preventDefault();
        setExploded((value) => !value);
      } else if (event.key === "!") {
        event.preventDefault();
        setExploded(false);
      } else if (event.key === "Escape") {
        setSelectedId(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    handleImport,
    doUndo,
    doRedo,
    doDuplicate,
    doDelete,
    requestView,
    selectedId,
  ]);

  const selectPart = (id: number) => {
    setSelectedId(id);
    setHidden((current) => {
      if (!current.has(id)) return current;
      const next = new Set(current);
      next.delete(id);
      return next;
    });
  };

  const toggleHidden = (id: number) => {
    setHidden((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const patchConnectors = (patch: Partial<ConnectorOptions>) => {
    setConnectors((current) => ({ ...current, ...patch }));
  };

  const axisIndex = AXIS_INDEX[axis];
  const cutMin = selectedPart ? selectedPart.min[axisIndex] : 0;
  const cutMax = selectedPart ? selectedPart.max[axisIndex] : 1;
  const cutSpan = Math.max(cutMax - cutMin, 0);
  const cutStep = Math.max(cutSpan / 400, 0.01);
  const cutProgress =
    cutSpan > 0 ? ((clamp(cutPosition, cutMin, cutMax) - cutMin) / cutSpan) * 100 : 50;
  const clampedCut = clamp(cutPosition, cutMin, cutMax);

  const planePreview = useMemo<PlanePreview | null>(() => {
    if (tab !== "cut" || !selectedPart) return null;
    return { axis, position: clampedCut, part: selectedPart };
  }, [tab, selectedPart, axis, clampedCut]);

  const usable = [
    Math.max(bed.x - 2 * bed.margin, 10),
    Math.max(bed.y - 2 * bed.margin, 10),
    Math.max(bed.z - 2 * bed.margin, 10),
  ];

  return (
    <div className="app">
      <header className="header">
        <div className="brand">
          <BrandMark />
          <div className="brand-text">
            <div className="brand-name">
              SUPER<em>TRUC</em> 2000
            </div>
            <div className="brand-sub">Atelier de découpe 3D</div>
          </div>
        </div>
        <div className="header-sep" />
        <div className="header-actions">
          <button
            type="button"
            className="btn primary"
            onClick={() => void handleImport()}
            disabled={!!busy}
            title="Importer un modèle (Ctrl+O)"
          >
            <IconImport size={15} />
            Importer
          </button>
          <button
            type="button"
            className="btn icon-only ghost"
            onClick={doUndo}
            disabled={ws.undoDepth === 0 || !!busy}
            title="Annuler (Ctrl+Z)"
          >
            <IconUndo size={16} />
          </button>
          <button
            type="button"
            className="btn icon-only ghost"
            onClick={doRedo}
            disabled={ws.redoDepth === 0 || !!busy}
            title="Rétablir (Ctrl+Maj+Z)"
          >
            <IconRedo size={16} />
          </button>
        </div>
        <div className="header-spacer" />
        <div className="header-meta">
          <span>
            Pièces <b>{parts.length}</b>
          </span>
          <span>
            Triangles <b>{fmtInt(ws.totalTriangles)}</b>
          </span>
          <span>
            Volume <b>{fmtVolume(ws.totalVolume)}</b>
          </span>
        </div>
      </header>

      <div className="main">
        <aside className="panel left">
          <div className="panel-head">
            <span className="panel-title">Pièces</span>
            <span className="count-badge">{parts.length}</span>
            <span className="header-spacer" />
            {parts.length > 0 ? (
              <button
                type="button"
                className="btn small ghost"
                onClick={doClear}
                disabled={!!busy}
                title="Vider l'atelier (annulable)"
              >
                Vider
              </button>
            ) : null}
          </div>
          <div className="panel-body">
            <button
              type="button"
              className="dropzone"
              style={{ width: "100%" }}
              onClick={() => void handleImport()}
              disabled={!!busy}
            >
              <div className="dz-icon">
                <IconImport size={22} />
              </div>
              <div className="dz-title">Importer un modèle 3D</div>
              <div className="dz-sub">
                STL, OBJ ou 3MF
                <br />
                ou glissez-déposez un fichier
              </div>
            </button>

            {parts.length === 0 ? (
              <div className="empty-hint">
                Aucune pièce dans l'atelier.
                <br />
                Importez un grand modèle, puis découpez-le en pièces imprimables.
              </div>
            ) : (
              <div className="parts-list" style={{ marginTop: 12 }}>
                {parts.map((part, index) => {
                  const color = partColorAt(index);
                  const isSelected = part.id === selectedId;
                  const isHidden = hidden.has(part.id);
                  return (
                    <div
                      key={part.id}
                      className={`part-row ${isSelected ? "selected" : ""}`}
                      onClick={() => selectPart(part.id)}
                      title={`${part.name} — ${fmtSize(part.size)} mm`}
                    >
                      <span
                        className="part-swatch"
                        style={{ background: isHidden ? "#3a444f" : color, color }}
                      />
                      <div className="part-main" style={{ opacity: isHidden ? 0.5 : 1 }}>
                        <div className="part-name">{part.name}</div>
                        <div className="part-meta">
                          {fmtSize(part.size)} mm · {fmtInt(part.triangles)} tris
                        </div>
                      </div>
                      <div
                        className="part-actions"
                        onClick={(event) => event.stopPropagation()}
                      >
                        <button
                          type="button"
                          className="icon-btn"
                          onClick={() => toggleHidden(part.id)}
                          title={isHidden ? "Afficher la pièce" : "Masquer la pièce"}
                        >
                          {isHidden ? <IconEyeOff size={14} /> : <IconEye size={14} />}
                        </button>
                        <button
                          type="button"
                          className="icon-btn"
                          onClick={() => doDuplicate(part.id)}
                          title="Dupliquer (Ctrl+D)"
                          disabled={!!busy}
                        >
                          <IconCopy size={14} />
                        </button>
                        <button
                          type="button"
                          className="icon-btn danger"
                          onClick={() => doDelete(part.id)}
                          title="Supprimer (Suppr)"
                          disabled={!!busy}
                        >
                          <IconTrash size={14} />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
          <div className="panel-foot">
            <span>
              <b>{parts.length}</b> pièce(s)
            </span>
            <span>
              <b>{fmtInt(ws.totalTriangles)}</b> triangles
            </span>
          </div>
        </aside>

        <Viewport
          parts={parts}
          bed={bed}
          selectedId={selectedId}
          hidden={hidden}
          plane={planePreview}
          command={command}
          exploded={exploded}
          onSelect={(id) => {
            if (id === null) setSelectedId(null);
            else selectPart(id);
          }}
        >
          <div className="viewport-overlay">
            <div className="viewport-top">
              <div className="chip-row">
                <span className="chip">
                  <b>{parts.length}</b> pièce(s)
                </span>
                {selectedPart ? (
                  <span className="chip accent">
                    Sélection <b>{selectedPart.name}</b>
                  </span>
                ) : null}
                {selectedPart ? (
                  <span className="chip">{fmtSize(selectedPart.size)} mm</span>
                ) : null}
                {parts.length > 0 && selectedId === null ? (
                  <span className="chip">Cliquez une pièce pour la sélectionner</span>
                ) : null}
              </div>
              <div className="view-actions">
                <button
                  type="button"
                  className={`overlay-btn${exploded ? " accent" : ""}`}
                  onClick={() => setExploded((value) => !value)}
                  title="Éclater / rassembler les pièces (E, ! pour rassembler)"
                >
                  {exploded ? "Rassembler" : "Éclater"}
                </button>
                <button
                  type="button"
                  className="overlay-btn"
                  onClick={() => requestView("fit")}
                  title="Recadrer la vue (F)"
                >
                  <IconFit size={13} />
                  Ajuster
                </button>
                <button
                  type="button"
                  className="overlay-btn"
                  onClick={() => requestView("iso")}
                >
                  Iso
                </button>
                <button
                  type="button"
                  className="overlay-btn"
                  onClick={() => requestView("top")}
                >
                  Dessus
                </button>
                <button
                  type="button"
                  className="overlay-btn"
                  onClick={() => requestView("front")}
                >
                  Face
                </button>
                <button
                  type="button"
                  className="overlay-btn"
                  onClick={() => requestView("right")}
                >
                  Droite
                </button>
              </div>
            </div>
            <div className="viewport-bottom">
              <div className="axis-legend">
                <span className="axis-dot" style={{ color: "#ff8a8a" }} title="Axe X">
                  X
                </span>
                <span className="axis-dot" style={{ color: "#84e08f" }} title="Axe Y">
                  Y
                </span>
                <span className="axis-dot" style={{ color: "#7db7ff" }} title="Axe Z">
                  Z
                </span>
                <span className="view-hint">
                  Plateau {fmt0(bed.x)} × {fmt0(bed.y)} × {fmt0(bed.z)} mm
                </span>
              </div>
              <div className="view-hint">
                Clic : sélectionner · Glisser : orbiter · Molette : zoomer · F :
                recadrer · E : éclater · ! : rassembler
              </div>
            </div>
          </div>
          {dragging ? (
            <div className="drag-overlay">
              Déposez votre modèle
              <span>STL, OBJ ou 3MF</span>
            </div>
          ) : null}
        </Viewport>

        <aside className="panel right">
          <div className="tools-tabs">
            {TAB_META.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`tool-tab ${tab === item.id ? "active" : ""}`}
                onClick={() => setTab(item.id)}
              >
                {item.icon}
                {item.label}
              </button>
            ))}
          </div>

          <div className="tool-scroll">
            {tab === "cut" ? (
              !selectedPart ? (
                <div className="section">
                  <div className="section-note">
                    Sélectionnez une pièce dans la liste ou dans la vue 3D pour
                    définir un plan de coupe.
                  </div>
                </div>
              ) : (
                <>
                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Plan de coupe</span>
                      <span className="badge">{selectedPart.name}</span>
                    </div>
                    <div className="segmented">
                      {AXES.map((item) => (
                        <button
                          key={item}
                          type="button"
                          className={axis === item ? "active accent" : ""}
                          onClick={() => setAxis(item)}
                        >
                          {item.toUpperCase()}
                        </button>
                      ))}
                    </div>
                    <div className="field" style={{ marginTop: 10 }}>
                      <div className="field-label">
                        <span>Position sur {axis.toUpperCase()}</span>
                        <b>{fmt2(clampedCut)} mm</b>
                      </div>
                      <input
                        type="range"
                        min={cutMin}
                        max={cutMax}
                        step={cutStep}
                        value={clampedCut}
                        disabled={cutSpan <= 1e-6}
                        onChange={(event) =>
                          setCutPosition(Number.parseFloat(event.target.value))
                        }
                        style={
                          {
                            "--range-progress": `${cutProgress}%`,
                          } as CSSProperties
                        }
                      />
                      <div className="range-scale">
                        <span>{fmt1(cutMin)}</span>
                        <span>{fmt1(cutMax)}</span>
                      </div>
                    </div>
                    <div className="input-with-unit" style={{ marginTop: 8 }}>
                      <input
                        className="input"
                        type="number"
                        step={0.1}
                        value={Number(clampedCut.toFixed(2))}
                        onChange={(event) => {
                          const parsed = Number.parseFloat(event.target.value);
                          if (Number.isFinite(parsed)) {
                            setCutPosition(clamp(parsed, cutMin, cutMax));
                          }
                        }}
                      />
                      <span className="unit">mm</span>
                    </div>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Connecteurs</span>
                      <span className="badge">
                        {connectors.enabled
                          ? connectors.auto
                            ? "Auto"
                            : `${connectors.count} tenon(s)`
                          : "Désactivés"}
                      </span>
                    </div>
                    <div className="toggle-row">
                      <div className="toggle-label">
                        <span>Tenons et logements</span>
                        <small>
                          Le côté négatif reçoit les tenons, le côté positif les
                          logements.
                        </small>
                      </div>
                      <Switch
                        on={connectors.enabled}
                        onToggle={() => patchConnectors({ enabled: !connectors.enabled })}
                        label="Activer les connecteurs"
                      />
                    </div>
                    {connectors.enabled ? (
                      <>
                        <div className="toggle-row">
                          <div className="toggle-label">
                            <span>Placement automatique</span>
                            <small>Répartit les tenons dans la section de coupe.</small>
                          </div>
                          <Switch
                            on={connectors.auto}
                            onToggle={() => patchConnectors({ auto: !connectors.auto })}
                            label="Placement automatique des connecteurs"
                          />
                        </div>
                        {!connectors.auto ? (
                          <div
                            style={{
                              display: "grid",
                              gridTemplateColumns: "1fr 1fr",
                              gap: 8,
                              marginTop: 8,
                            }}
                          >
                            <NumberField
                              label="Nombre"
                              value={connectors.count}
                              min={1}
                              max={12}
                              step={1}
                              unit=""
                              onChange={(value) =>
                                patchConnectors({
                                  count: Math.round(clamp(value, 1, 12)),
                                })
                              }
                            />
                            <NumberField
                              label="Rayon"
                              value={connectors.radius}
                              min={0.5}
                              max={20}
                              step={0.5}
                              onChange={(value) =>
                                patchConnectors({ radius: clamp(value, 0.5, 20) })
                              }
                            />
                            <NumberField
                              label="Profondeur"
                              value={connectors.depth}
                              min={1}
                              max={50}
                              step={1}
                              onChange={(value) =>
                                patchConnectors({ depth: clamp(value, 1, 50) })
                              }
                            />
                            <NumberField
                              label="Jeu"
                              value={connectors.clearance}
                              min={0}
                              max={1}
                              step={0.05}
                              onChange={(value) =>
                                patchConnectors({ clearance: clamp(value, 0, 1) })
                              }
                            />
                          </div>
                        ) : null}
                      </>
                    ) : null}
                    <button
                      type="button"
                      className="btn primary block"
                      style={{ marginTop: 12 }}
                      onClick={doCut}
                      disabled={!!busy}
                    >
                      <IconScissors size={15} />
                      Découper la pièce
                    </button>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Repères</span>
                    </div>
                    <div className="stat-grid">
                      <div className="stat">
                        <div className="k">Position</div>
                        <div className="v">
                          {fmt1(clampedCut)}
                          <small>mm</small>
                        </div>
                      </div>
                      <div className="stat">
                        <div className="k">Section</div>
                        <div className="v">
                          {fmt1(cutSpan)}
                          <small>mm</small>
                        </div>
                      </div>
                    </div>
                    <div className="section-note" style={{ marginTop: 8 }}>
                      La découpe crée deux pièces étanches avec un bouchon
                      triangulé. Utilisez Ctrl+Z pour revenir en arrière.
                    </div>
                  </section>
                </>
              )
            ) : null}

            {tab === "auto" ? (
              <>
                <section className="section">
                  <div className="section-head">
                    <span className="section-title">Volume d'impression</span>
                    <span className={`badge ${bedDirty ? "warn" : ""}`}>
                      {bedDirty ? "Modifié" : "Actif"}
                    </span>
                  </div>
                  <div className="input-group">
                    <NumberField
                      label="X"
                      value={bedDraft.x}
                      min={20}
                      max={5000}
                      onChange={(value) => setBedDraft((bed) => ({ ...bed, x: value }))}
                    />
                    <NumberField
                      label="Y"
                      value={bedDraft.y}
                      min={20}
                      max={5000}
                      onChange={(value) => setBedDraft((bed) => ({ ...bed, y: value }))}
                    />
                    <NumberField
                      label="Z"
                      value={bedDraft.z}
                      min={20}
                      max={5000}
                      onChange={(value) => setBedDraft((bed) => ({ ...bed, z: value }))}
                    />
                  </div>
                  <div className="field" style={{ marginTop: 10 }}>
                    <div className="field-label">
                      <span>Marge de sécurité</span>
                      <b>{fmt1(bedDraft.margin)} mm</b>
                    </div>
                    <input
                      type="range"
                      min={0}
                      max={20}
                      step={0.5}
                      value={bedDraft.margin}
                      onChange={(event) =>
                        setBedDraft((bed) => ({
                          ...bed,
                          margin: Number.parseFloat(event.target.value),
                        }))
                      }
                      style={
                        {
                          "--range-progress": `${(bedDraft.margin / 20) * 100}%`,
                        } as CSSProperties
                      }
                    />
                    <div className="range-scale">
                      <span>0</span>
                      <span>20</span>
                    </div>
                  </div>
                  <button
                    type="button"
                    className="btn block"
                    style={{ marginTop: 10 }}
                    onClick={applyBed}
                    disabled={!bedDirty || !!busy}
                  >
                    Appliquer le plateau
                  </button>
                  <div className="section-note" style={{ marginTop: 8 }}>
                    Zone utile : <b>{fmtSize(usable)}</b> mm.
                  </div>
                </section>

                <section className="section">
                  <div className="section-head">
                    <span className="section-title">Découpe automatique</span>
                    <span className="badge">
                      {autoConnectors ? "Connecteurs" : "Sans connecteurs"}
                    </span>
                  </div>
                  <div className="segmented">
                    <button
                      type="button"
                      className={autoScope === "all" ? "active accent" : ""}
                      onClick={() => setAutoScope("all")}
                    >
                      Toutes les pièces
                    </button>
                    <button
                      type="button"
                      className={autoScope === "selected" ? "active accent" : ""}
                      onClick={() => setAutoScope("selected")}
                      disabled={!selectedPart}
                    >
                      Sélection
                    </button>
                  </div>
                  <div className="toggle-row" style={{ marginTop: 8 }}>
                    <div className="toggle-label">
                      <span>Connecteurs automatiques</span>
                      <small>
                        Tenons de 6 mm de profondeur ajoutés à chaque coupe.
                      </small>
                    </div>
                    <Switch
                      on={autoConnectors}
                      onToggle={() => setAutoConnectors((value) => !value)}
                      label="Connecteurs automatiques"
                    />
                  </div>
                  <div className="piece-estimate" style={{ marginTop: 10 }}>
                    <span>Pièces estimées</span>
                    <b>{autoEstimate}</b>
                  </div>
                  {autoEstimate > MAX_PIECES ? (
                    <div
                      className="section-note"
                      style={{ marginTop: 6, color: "var(--danger)" }}
                    >
                      Maximum {MAX_PIECES} pièces. Augmentez le plateau ou réduisez
                      le modèle.
                    </div>
                  ) : null}
                  <button
                    type="button"
                    className="btn primary block"
                    style={{ marginTop: 10 }}
                    onClick={doAutoSplit}
                    disabled={parts.length === 0 || autoEstimate > MAX_PIECES || !!busy}
                  >
                    <IconAuto size={15} />
                    Lancer la découpe auto
                  </button>
                  <div className="section-note" style={{ marginTop: 8 }}>
                    Les plans sont répartis uniformément sur X, Y et Z d'après la
                    zone utile du plateau.
                    {bedDirty ? " Appliquez d'abord le plateau modifié." : ""}
                  </div>
                </section>

                <section className="section">
                  <div className="section-head">
                    <span className="section-title">Disposition</span>
                    <span className="badge">{fmt1(arrangeGap)} mm</span>
                  </div>
                  <div className="field">
                    <div className="field-label">
                      <span>Espacement entre pièces</span>
                      <b>{fmt1(arrangeGap)} mm</b>
                    </div>
                    <input
                      type="range"
                      min={0}
                      max={20}
                      step={1}
                      value={arrangeGap}
                      onChange={(event) =>
                        setArrangeGap(Number.parseFloat(event.target.value))
                      }
                      style={
                        {
                          "--range-progress": `${(arrangeGap / 20) * 100}%`,
                        } as CSSProperties
                      }
                    />
                    <div className="range-scale">
                      <span>0</span>
                      <span>20</span>
                    </div>
                  </div>
                  <button
                    type="button"
                    className="btn block"
                    style={{ marginTop: 10 }}
                    onClick={doArrange}
                    disabled={parts.length === 0 || !!busy}
                  >
                    <IconFit size={15} />
                    Ranger sur le plateau
                  </button>
                  <div className="section-note" style={{ marginTop: 8 }}>
                    Les pièces sont rangées par volume décroissant, alignées sur le
                    plateau.
                  </div>
                </section>
              </>
            ) : null}

            {tab === "edit" ? (
              !selectedPart ? (
                <div className="section">
                  <div className="section-note">
                    Sélectionnez une pièce pour la déplacer, la tourner, la
                    redimensionner ou la réparer.
                  </div>
                </div>
              ) : (
                <>
                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Pièce sélectionnée</span>
                      <span className="badge">{selectedPart.name}</span>
                    </div>
                    <div className="stat-grid">
                      <div className="stat">
                        <div className="k">Triangles</div>
                        <div className="v">{fmtInt(selectedPart.triangles)}</div>
                      </div>
                      <div className="stat">
                        <div className="k">Volume</div>
                        <div className="v">{fmtVolume(selectedPart.volume)}</div>
                      </div>
                      <div className="stat">
                        <div className="k">Emprise</div>
                        <div className="v" style={{ fontSize: 11 }}>
                          {fmtSize(selectedPart.size)}
                          <small>mm</small>
                        </div>
                      </div>
                      <div className="stat">
                        <div className="k">Surface</div>
                        <div className="v">
                          {(selectedPart.area / 100).toLocaleString("fr-FR", {
                            maximumFractionDigits: 1,
                          })}
                          <small>cm²</small>
                        </div>
                      </div>
                    </div>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Déplacer</span>
                    </div>
                    <div className="input-group">
                      <NumberField label="X" value={tx} onChange={setTx} />
                      <NumberField label="Y" value={ty} onChange={setTy} />
                      <NumberField label="Z" value={tz} onChange={setTz} />
                    </div>
                    <button
                      type="button"
                      className="btn block"
                      style={{ marginTop: 10 }}
                      onClick={doTranslate}
                      disabled={!!busy}
                    >
                      Déplacer la pièce
                    </button>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Pivoter</span>
                    </div>
                    <div className="segmented">
                      {AXES.map((item) => (
                        <button
                          key={item}
                          type="button"
                          className={rotationAxis === item ? "active accent" : ""}
                          onClick={() => setRotationAxis(item)}
                        >
                          {item.toUpperCase()}
                        </button>
                      ))}
                    </div>
                    <div className="field" style={{ marginTop: 10 }}>
                      <div className="field-label">
                        <span>Angle</span>
                      </div>
                      <div className="input-with-unit">
                        <input
                          className="input"
                          type="number"
                          step={1}
                          value={rotationDegrees}
                          onChange={(event) =>
                            setRotationDegrees(
                              parseNumber(event.target.value, 0),
                            )
                          }
                        />
                        <span className="unit">°</span>
                      </div>
                    </div>
                    <button
                      type="button"
                      className="btn block"
                      style={{ marginTop: 10 }}
                      onClick={doRotate}
                      disabled={!!busy}
                    >
                      Pivoter de {fmt1(rotationDegrees)}°
                    </button>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Dimensions</span>
                    </div>
                    <div className="field">
                      <div className="field-label">
                        <span>Échelle</span>
                        <b>{fmt1(scalePercent)} %</b>
                      </div>
                      <input
                        type="range"
                        min={5}
                        max={400}
                        step={1}
                        value={scalePercent}
                        onChange={(event) =>
                          setScalePercent(Number.parseFloat(event.target.value))
                        }
                        style={
                          {
                            "--range-progress": `${((scalePercent - 5) / 395) * 100}%`,
                          } as CSSProperties
                        }
                      />
                      <div className="range-scale">
                        <span>5 %</span>
                        <span>400 %</span>
                      </div>
                    </div>
                    <button
                      type="button"
                      className="btn block"
                      style={{ marginTop: 8 }}
                      onClick={doScale}
                      disabled={!!busy}
                    >
                      Appliquer l'échelle
                    </button>
                    <div className="divider" />
                    <NumberField
                      label="Hauteur cible"
                      value={targetHeight}
                      min={1}
                      max={5000}
                      step={1}
                      onChange={setTargetHeight}
                    />
                    <button
                      type="button"
                      className="btn block"
                      style={{ marginTop: 8 }}
                      onClick={doSetHeight}
                      disabled={!!busy}
                    >
                      Définir la hauteur
                    </button>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Placement</span>
                    </div>
                    <div className="btn-row">
                      <button
                        type="button"
                        className="btn small"
                        onClick={() =>
                          applyTransform({ kind: "fitBed" }, "Ajustement au plateau…")
                        }
                        disabled={!!busy}
                      >
                        Ajuster
                      </button>
                      <button
                        type="button"
                        className="btn small"
                        onClick={() =>
                          applyTransform(
                            { kind: "dropToBed" },
                            "Pose sur le plateau…",
                          )
                        }
                        disabled={!!busy}
                      >
                        Poser
                      </button>
                      <button
                        type="button"
                        className="btn small"
                        onClick={() =>
                          applyTransform({ kind: "center" }, "Centrage…")
                        }
                        disabled={!!busy}
                      >
                        Centrer
                      </button>
                    </div>
                    <div className="btn-row" style={{ marginTop: 6 }}>
                      {AXES.map((item) => (
                        <button
                          key={item}
                          type="button"
                          className="btn small ghost"
                          onClick={() =>
                            applyTransform(
                              { kind: "mirror", axis: item },
                              `Miroir ${item.toUpperCase()}…`,
                            )
                          }
                          disabled={!!busy}
                        >
                          Miroir {item.toUpperCase()}
                        </button>
                      ))}
                    </div>
                    <button
                      type="button"
                      className="btn block teal"
                      style={{ marginTop: 8 }}
                      onClick={() => requestView("fit")}
                    >
                      <IconFit size={14} />
                      Cadrer sur la pièce
                    </button>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Évidement</span>
                      <span className="badge warn">Parois</span>
                    </div>
                    <div className="section-note">
                      Vide l'intérieur de la pièce et ne conserve qu'une coque.
                      Le modèle doit être étanche pour un résultat propre.
                    </div>
                    <NumberField
                      label="Épaisseur des parois"
                      value={wallThickness}
                      min={0.2}
                      max={100}
                      step={0.2}
                      onChange={setWallThickness}
                    />
                    <button
                      type="button"
                      className="btn block"
                      style={{ marginTop: 10 }}
                      onClick={doHollow}
                      disabled={!!busy || !selectedPart}
                    >
                      <IconRefresh size={15} />
                      Évider la pièce
                    </button>
                  </section>

                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Réparation</span>
                      <span className="badge warn">Maillage</span>
                    </div>
                    <div className="section-note">
                      Soude les sommets confondus et reconstruit un maillage propre
                      avant impression.
                    </div>
                    <button
                      type="button"
                      className="btn block"
                      style={{ marginTop: 10 }}
                      onClick={doRepair}
                      disabled={!!busy}
                    >
                      <IconRefresh size={15} />
                      Réparer le maillage
                    </button>
                  </section>
                </>
              )
            ) : null}

            {tab === "export" ? (
              <>
                <section className="section">
                  <div className="section-head">
                    <span className="section-title">Format</span>
                  </div>
                  <div className="segmented">
                    {(["stl", "obj", "3mf"] as ExportFormat[]).map((item) => (
                      <button
                        key={item}
                        type="button"
                        className={format === item ? "active accent" : ""}
                        onClick={() => setFormat(item)}
                      >
                        {item.toUpperCase()}
                      </button>
                    ))}
                  </div>
                  <div className="section-note" style={{ marginTop: 8 }}>
                    {EXPORT_HINTS[format]}
                  </div>
                </section>

                <section className="section">
                  <div className="section-head">
                    <span className="section-title">Portée</span>
                    <span className="badge">{exportCount} fichier(s)</span>
                  </div>
                  <div className="segmented">
                    <button
                      type="button"
                      className={exportScope === "all" ? "active accent" : ""}
                      onClick={() => setExportScope("all")}
                    >
                      Tout l'atelier
                    </button>
                    <button
                      type="button"
                      className={exportScope === "selected" ? "active accent" : ""}
                      onClick={() => setExportScope("selected")}
                      disabled={!selectedPart}
                    >
                      Sélection
                    </button>
                  </div>
                  <div className="section-note" style={{ marginTop: 8 }}>
                    {exportScope === "selected"
                      ? selectedPart
                        ? `Pièce : ${selectedPart.name}`
                        : "Aucune pièce sélectionnée."
                      : `${parts.length} pièce(s) seront exportées.`}
                  </div>
                </section>

                <section className="section">
                  <div className="section-head">
                    <span className="section-title">Destination</span>
                  </div>
                  <div
                    className="input"
                    style={{
                      height: "auto",
                      minHeight: 34,
                      padding: "8px 9px",
                      fontSize: 10.5,
                      lineHeight: 1.45,
                      color: exportPath ? "var(--text-dim)" : "var(--muted)",
                      wordBreak: "break-all",
                    }}
                  >
                    {exportPath ||
                      (exportNeedsDirectory
                        ? "Dossier non choisi"
                        : "Fichier non choisi")}
                  </div>
                  <button
                    type="button"
                    className="btn block"
                    style={{ marginTop: 8 }}
                    onClick={() => void chooseExportPath()}
                    disabled={!!busy}
                  >
                    <IconFolder size={15} />
                    {exportNeedsDirectory ? "Choisir un dossier" : "Choisir un fichier"}
                  </button>
                  <button
                    type="button"
                    className="btn primary block"
                    style={{ marginTop: 8 }}
                    onClick={() => void doExport()}
                    disabled={
                      parts.length === 0 ||
                      (exportScope === "selected" && !selectedPart) ||
                      !!busy
                    }
                  >
                    <IconExport size={15} />
                    Exporter
                  </button>
                  <div className="section-note" style={{ marginTop: 8 }}>
                    {format === "stl" && exportCount > 1
                      ? "Un fichier STL sera créé par pièce dans le dossier choisi."
                      : "Le fichier sera écrit à l'emplacement choisi."}
                  </div>
                </section>

                {lastExport ? (
                  <section className="section">
                    <div className="section-head">
                      <span className="section-title">Dernier export</span>
                      <span className="badge">{lastExport.files.length} fichier(s)</span>
                    </div>
                    <ul className="help-list">
                      {lastExport.files.slice(0, 8).map((file) => (
                        <li key={file}>
                          <span className="num">✓</span>
                          <span style={{ wordBreak: "break-all" }}>{file}</span>
                        </li>
                      ))}
                    </ul>
                    {lastExport.files.length > 8 ? (
                      <div className="section-note" style={{ marginTop: 6 }}>
                        … et {lastExport.files.length - 8} autre(s) fichier(s).
                      </div>
                    ) : null}
                  </section>
                ) : null}
              </>
            ) : null}
          </div>
        </aside>
      </div>

      <footer className="statusbar">
        <span className="status-dot" />
        <span>{status}</span>
        <span className="status-spacer" />
        <span>
          Plateau <b>{fmt0(bed.x)} × {fmt0(bed.y)} × {fmt0(bed.z)}</b> mm
        </span>
        <span>
          Zone utile <b>{fmtSize(usable)}</b> mm
        </span>
        <span>Ctrl+O import · Ctrl+Z annuler · Suppr supprimer</span>
      </footer>

      <div className="toast-stack">
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast ${toast.kind}`}>
            <span className="toast-icon">
              <ToastGlyph kind={toast.kind} />
            </span>
            <div className="toast-content">
              <strong>{toast.title}</strong>
              {toast.details && toast.details.length > 0 ? (
                toast.details.length === 1 ? (
                  <span>{toast.details[0]}</span>
                ) : (
                  <ul>
                    {toast.details.map((detail, index) => (
                      <li key={`${toast.id}-${index}`}>{detail}</li>
                    ))}
                  </ul>
                )
              ) : null}
            </div>
            <button
              type="button"
              className="toast-close"
              onClick={() =>
                setToasts((current) =>
                  current.filter((item) => item.id !== toast.id),
                )
              }
              aria-label="Fermer la notification"
            >
              <IconClose size={14} />
            </button>
          </div>
        ))}
      </div>

      {busy ? (
        <div className="busy-veil">
          <div className="busy-card">
            <span className="loader-ring" />
            {busy}
          </div>
        </div>
      ) : null}
    </div>
  );
}
