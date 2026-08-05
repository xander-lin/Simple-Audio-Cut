import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import Icon from "./Icon";

export interface DenoiseConfig {
  enabled: boolean;
  executablePath: string;
  modelRoot: string;
  device: string;
  highSampleRateModel: string;
  lowSampleRateModel: string;
}

interface DenoiseModelInfo {
  id: string;
  sampleRate: number | null;
  ready: boolean;
  detail: string;
}

interface DenoiseDeviceInfo {
  id: string;
  label: string;
}

interface DenoiseCheck {
  id: string;
  label: string;
  state: "ready" | "warning" | "error";
  detail: string;
}

export interface DenoiseProviderStatus {
  providerId: string;
  providerName: string;
  configured: DenoiseConfig;
  effective: DenoiseConfig;
  environmentOverrides: string[];
  models: DenoiseModelInfo[];
  devices: DenoiseDeviceInfo[];
  checks: DenoiseCheck[];
  ready: boolean;
  summary: string;
}

interface DenoiseSettingsProps {
  visible: boolean;
  onClose: () => void;
  onDialogClosed: () => void;
  onStatusChange: React.Dispatch<React.SetStateAction<DenoiseProviderStatus | null>>;
  onSaved: (status: DenoiseProviderStatus) => void;
}

function selectedPath(selection: string | string[] | null) {
  return typeof selection === "string" ? selection : null;
}

function modelOptions(models: DenoiseModelInfo[], current: string, highRate: boolean) {
  const preferred = models.filter((model) => model.ready && (
    model.sampleRate === null || (highRate ? model.sampleRate >= 44_100 : model.sampleRate < 44_100)
  ));
  const fallback = models.filter((model) => model.ready && !preferred.includes(model));
  const values = [current, ...preferred.map((model) => model.id), ...fallback.map((model) => model.id)];
  return [...new Set(values.filter(Boolean))];
}

interface SettingsChoiceOption {
  value: string;
  label: string;
}

interface SettingsChoiceProps {
  className?: string;
  label: string;
  value: string;
  options: SettingsChoiceOption[];
  onChange: (value: string) => void;
}

function SettingsChoice({ className = "", label, value, options, onChange }: SettingsChoiceProps) {
  const id = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [open, setOpen] = useState(false);
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const selected = options[selectedIndex];

  useEffect(() => {
    if (open) optionRefs.current[selectedIndex]?.focus();
  }, [open, selectedIndex]);

  useEffect(() => {
    if (!open) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [open]);

  const focusOption = (index: number) => {
    if (options.length === 0) return;
    optionRefs.current[(index + options.length) % options.length]?.focus();
  };

  return <div ref={rootRef} className={`settings-choice ${className} ${open ? "is-open" : ""}`}>
    <span id={`${id}-label`}>{label}</span>
    <button
      ref={triggerRef}
      type="button"
      className="settings-choice-trigger"
      role="combobox"
      aria-controls={`${id}-listbox`}
      aria-expanded={open}
      aria-haspopup="listbox"
      aria-labelledby={`${id}-label ${id}-value`}
      disabled={options.length === 0}
      onClick={() => setOpen((current) => !current)}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          setOpen(true);
        } else if (event.key === "Escape" && open) {
          event.preventDefault();
          setOpen(false);
        }
      }}
    >
      <span id={`${id}-value`} className="settings-choice-value">{selected?.label ?? "No options available"}</span>
      <span className="settings-choice-arrow" aria-hidden="true" />
    </button>
    {open && <div id={`${id}-listbox`} className="settings-choice-menu" role="listbox" aria-labelledby={`${id}-label`}>
      {options.map((option, index) => <button
        key={option.value}
        ref={(node) => { optionRefs.current[index] = node; }}
        type="button"
        className="settings-choice-option"
        role="option"
        aria-selected={option.value === value}
        onClick={() => {
          onChange(option.value);
          setOpen(false);
          triggerRef.current?.focus();
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            focusOption(index + 1);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            focusOption(index - 1);
          } else if (event.key === "Home") {
            event.preventDefault();
            focusOption(0);
          } else if (event.key === "End") {
            event.preventDefault();
            focusOption(options.length - 1);
          } else if (event.key === "Escape") {
            event.preventDefault();
            setOpen(false);
            triggerRef.current?.focus();
          }
        }}
      >{option.label}</button>)}
    </div>}
  </div>;
}

export default function DenoiseSettings({
  visible,
  onClose,
  onDialogClosed,
  onStatusChange,
  onSaved,
}: DenoiseSettingsProps) {
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const [status, setStatus] = useState<DenoiseProviderStatus | null>(null);
  const [draft, setDraft] = useState<DenoiseConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const applyStatus = (next: DenoiseProviderStatus, updateDraft = true) => {
    setStatus(next);
    if (updateDraft) setDraft(next.configured);
    onStatusChange(next);
  };

  useEffect(() => {
    if (!visible) return;
    let disposed = false;
    setBusy(true);
    setError(null);
    void invoke<DenoiseProviderStatus>("denoise_provider_status")
      .then((next) => {
        if (disposed) return;
        setStatus(next);
        setDraft(next.configured);
        onStatusChange(next);
      })
      .catch((reason) => {
        if (!disposed) setError(String(reason));
      })
      .finally(() => {
        if (!disposed) setBusy(false);
      });
    return () => {
      disposed = true;
    };
  }, [visible, onStatusChange]);

  useEffect(() => {
    if (!visible) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [visible, onClose]);

  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => closeButtonRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [visible]);

  if (!visible) return null;

  const update = (partial: Partial<DenoiseConfig>) => {
    setDraft((current) => current ? { ...current, ...partial } : current);
    setError(null);
  };

  const inspect = async () => {
    if (!draft || busy) return;
    setBusy(true);
    setError(null);
    try {
      applyStatus(await invoke<DenoiseProviderStatus>("inspect_denoise_config", { config: draft }));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!draft || busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = await invoke<DenoiseProviderStatus>("save_denoise_config", { config: draft });
      applyStatus(next);
      onSaved(next);
      onClose();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const chooseExecutable = async () => {
    try {
      const path = selectedPath(await open({
        multiple: false,
        directory: false,
        title: "Choose the Python executable from your ClearVoice environment",
      }));
      if (path) update({ executablePath: path });
    } catch (reason) {
      setError(String(reason));
    } finally {
      onDialogClosed();
    }
  };

  const chooseModelRoot = async () => {
    try {
      const path = selectedPath(await open({
        multiple: false,
        directory: true,
        title: "Choose your ClearVoice model library",
      }));
      if (path) update({ modelRoot: path });
    } catch (reason) {
      setError(String(reason));
    } finally {
      onDialogClosed();
    }
  };

  const highRateModels = status && draft
    ? modelOptions(status.models, draft.highSampleRateModel, true)
    : [];
  const lowRateModels = status && draft
    ? modelOptions(status.models, draft.lowSampleRateModel, false)
    : [];
  const providerConfigurable = status?.providerId !== "disabled";

  return <div className="settings-layer">
    <button type="button" className="settings-scrim" onClick={onClose} disabled={busy} aria-label="Close denoising settings" />
    <aside className="settings-drawer" role="dialog" aria-modal="true" aria-labelledby="denoise-settings-title">
      <header className="settings-header">
        <div>
          <p className="section-kicker">Processing provider</p>
          <h2 id="denoise-settings-title">Denoising</h2>
        </div>
        <button ref={closeButtonRef} type="button" className="icon-button close-drawer-button" onClick={onClose} disabled={busy} aria-label="Close denoising settings">×</button>
      </header>

      <div className="settings-scroll">
        {busy && !status && <div className="settings-loading"><span className="activity-spinner" />Inspecting the local environment…</div>}
        {error && <div className="settings-error" role="alert"><Icon name="warning" />{error}</div>}
        {status && draft && <>
          <section className={`provider-summary ${status.ready ? "is-ready" : "needs-setup"}`}>
            <div className="provider-summary-heading">
              <span className="provider-mark">{status.ready ? <Icon name="check" /> : <Icon name="warning" />}</span>
              <div><strong>{status.providerName}</strong><span>{status.ready ? "Ready" : "Needs setup"}</span></div>
            </div>
            <p>{status.summary}</p>
          </section>

          <section className="settings-section">
            <div className="setting-row setting-toggle-row">
              <div><strong>Automatic denoising</strong><span>New media stays usable while this runs in the background.</span></div>
              <label className="toggle-switch">
                <input type="checkbox" checked={draft.enabled} disabled={!providerConfigurable} onChange={(event) => update({ enabled: event.target.checked })} />
                <span />
              </label>
            </div>
          </section>

          {providerConfigurable && <section className="settings-section">
            <div className="settings-section-heading"><span>1</span><div><strong>Runtime</strong><small>Use an environment you own. The app never installs or changes Python packages.</small></div></div>
            <label className="path-field">
              <span>Python executable</span>
              <div><input value={draft.executablePath} onChange={(event) => update({ executablePath: event.target.value })} placeholder="/path/to/environment/bin/python" spellCheck={false} /><button type="button" aria-label="Browse for Python executable" onClick={() => void chooseExecutable()}><Icon name="file" />Browse</button></div>
            </label>
          </section>}

          {providerConfigurable && <section className="settings-section">
            <div className="settings-section-heading"><span>2</span><div><strong>Model library</strong><small>Select the root folder, its checkpoints folder, or one installed model folder.</small></div></div>
            <label className="path-field">
              <span>Library folder</span>
              <div><input value={draft.modelRoot} onChange={(event) => update({ modelRoot: event.target.value })} placeholder="/path/to/model-library" spellCheck={false} /><button type="button" aria-label="Browse for model library" onClick={() => void chooseModelRoot()}><Icon name="folder" />Browse</button></div>
            </label>
            <div className="model-count">{status.models.length
              ? `${status.models.filter((model) => model.ready).length} of ${status.models.length} discovered models are complete`
              : "No models discovered yet"}</div>
          </section>}

          {providerConfigurable && <section className="settings-section">
            <div className="settings-section-heading"><span>3</span><div><strong>Routing</strong><small>Choose which installed model handles each audio family.</small></div></div>
            <div className="settings-grid">
              <SettingsChoice label="44.1 / 48 kHz audio" value={draft.highSampleRateModel} options={highRateModels.map((model) => ({ value: model, label: model }))} onChange={(highSampleRateModel) => update({ highSampleRateModel })} />
              <SettingsChoice label="Low-rate audio" value={draft.lowSampleRateModel} options={lowRateModels.map((model) => ({ value: model, label: model }))} onChange={(lowSampleRateModel) => update({ lowSampleRateModel })} />
              <SettingsChoice className="device-field" label="Processing device" value={draft.device} options={status.devices.map((device) => ({ value: device.id, label: device.label }))} onChange={(device) => update({ device })} />
            </div>
          </section>}

          {status.environmentOverrides.length > 0 && <section className="override-note">
            <Icon name="warning" /><div><strong>Launch overrides are active</strong><p>{status.environmentOverrides.join(", ")} currently come from environment variables. Saved values remain available but do not take effect until those overrides are removed.</p></div>
          </section>}

          <section className="settings-section validation-section">
            <div className="settings-section-heading"><span>4</span><div><strong>Validation</strong><small>Checks run locally and do not download or upload anything.</small></div></div>
            <div className="check-list">{status.checks.map((check) => <div key={check.id} className={`check-row is-${check.state}`}>
              <span className="check-icon">{check.state === "ready" ? <Icon name="check" /> : <Icon name="warning" />}</span>
              <div><strong>{check.label}</strong><span>{check.detail}</span></div>
            </div>)}</div>
          </section>
        </>}
      </div>

      <footer className="settings-footer">
        <button type="button" className="secondary-button" onClick={() => void inspect()} disabled={!draft || busy || !providerConfigurable}>{busy ? "Checking…" : "Check setup"}</button>
        <button type="button" className="primary-button" onClick={() => void save()} disabled={!draft || busy || !providerConfigurable}>{busy ? "Working…" : "Save settings"}</button>
      </footer>
    </aside>
  </div>;
}
