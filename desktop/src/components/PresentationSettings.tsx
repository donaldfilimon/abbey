interface Props {
  material: "auto" | "glass" | "solid";
  density: "comfortable" | "compact";
  setMaterial: (value: Props["material"]) => void;
  setDensity: (value: Props["density"]) => void;
  resetPresentation: () => void;
}

export function PresentationSettings({ material, density, setMaterial, setDensity, resetPresentation }: Props) {
  return (
    <>
      <section className="card settings-section">
        <div>
          <h2>Window material</h2>
          <p className="muted">Automatic uses a glass-inspired webview material on macOS and solid surfaces elsewhere. Reduced transparency and high contrast always take priority.</p>
        </div>
        <div className="segmented" role="group" aria-label="Window material">
          {(["auto", "glass", "solid"] as const).map((value) => (
            <button key={value} aria-pressed={material === value} onClick={() => setMaterial(value)}>
              {value === "auto" ? "Automatic" : value === "glass" ? "Glass" : "Solid"}
            </button>
          ))}
        </div>
      </section>
      <section className="card settings-section">
        <div><h2>Interface density</h2><p className="muted">Comfortable spacing by default; compact navigation when you need more room.</p></div>
        <div className="segmented" role="group" aria-label="Interface density">
          {(["comfortable", "compact"] as const).map((value) => (
            <button key={value} aria-pressed={density === value} onClick={() => setDensity(value)}>
              {value === "comfortable" ? "Comfortable" : "Compact"}
            </button>
          ))}
        </div>
      </section>
      <section className="card">
        <h2>Presentation defaults</h2>
        <p>Restore system appearance, automatic material, comfortable spacing, and the expanded sidebar. Conversations, model settings, and daemon state are not changed.</p>
        <button type="button" onClick={resetPresentation}>Restore presentation defaults</button>
      </section>
      <section className="card">
        <h2>Startup, background activity, and tools</h2>
        <p>Opening this window does not enable login startup or a background service. This app does not currently register either. Closing the window is not a request to cancel daemon-owned work.</p>
        <p>Desktop appearance settings stay in this webview. They do not change CLI/TUI providers, model routes, OS permissions, or tool grants. Model downloads and loading remain explicit actions in Models; tool and automation authority is not enabled by a visual preference.</p>
      </section>
    </>
  );
}
