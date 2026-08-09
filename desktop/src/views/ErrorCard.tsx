import type { IpcError } from "../ipc/generated";

const KIND_LABEL: Record<IpcError["kind"], string> = {
  configuration: "Daemon configuration error",
  transport: "Cannot reach Abbey",
  protocol: "Protocol mismatch",
  rejected: "Request rejected",
  unsupported_platform: "Unsupported platform",
};

export function ErrorCard({ error }: { error: IpcError }) {
  return (
    <div className="card error">
      <strong>{KIND_LABEL[error.kind]}</strong>
      <p className="claim-note">{error.message}</p>
      {error.remedy !== null && <p className="claim-note">{error.remedy}</p>}
    </div>
  );
}
