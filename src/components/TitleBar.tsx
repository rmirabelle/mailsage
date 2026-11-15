import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DownloadSimple, Info } from "@phosphor-icons/react";
import { WindowControls } from "./WindowControls";

interface Props {
  updateAvailable: boolean;
  onAbout: () => void;
}

/** The window's title bar: the app name, the update button when an update is ready, the About button, and the window controls. */
export function TitleBar({ updateAvailable, onAbout }: Props) {
  return (
    <header
      className="titlebar"
      data-tauri-drag-region
      onDoubleClick={() => isTauri() && getCurrentWindow().toggleMaximize()}
    >
      <div className="brand" data-tauri-drag-region>
        <img src="/icon.ico" alt="" />
        <span>Mail Sage</span>
        {/* Only the dev build shows this, so it is never mixed up with the installed app. */}
        {import.meta.env.DEV && <span className="brand-dev">DEV</span>}
      </div>
      {updateAvailable && (
        <button className="titlebar-update-button" onClick={onAbout} title="A new Mail Sage version is available">
          <DownloadSimple size={14} weight="bold" /> Update available
        </button>
      )}
      <div className="titlebar-spacer" data-tauri-drag-region />
      <button
        className="titlebar-icon-button"
        onClick={onAbout}
        onDoubleClick={(event) => event.stopPropagation()}
        aria-label="About Mail Sage"
        title="About Mail Sage"
      >
        <Info size={16} />
      </button>
      <WindowControls />
    </header>
  );
}
