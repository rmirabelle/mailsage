import { useRef, type KeyboardEvent, type PointerEvent } from "react";

const KEY_STEP = 16;

type Props = {
  label: string;
  /** The distance of the divider from the left side of the panes, in pixels. */
  position: number;
  /** The width of the column that this divider resizes. */
  width: number;
  min: number;
  max: number;
  onResize: (width: number) => void;
  /** The resize is complete: save the widths. */
  onDone: () => void;
  onReset: () => void;
};

/**
 * The line between two panes. The user drags it to resize the column on its
 * left. The arrow keys do the same, and a double-click gives the default width.
 */
export function PaneDivider({ label, position, width, min, max, onResize, onDone, onReset }: Props) {
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    /* The capture keeps the pointer events here while the pointer is above the preview frame. */
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { startX: event.clientX, startWidth: width };
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current) onResize(drag.current.startWidth + event.clientX - drag.current.startX);
  };

  const endDrag = () => {
    if (!drag.current) return;
    drag.current = null;
    onDone();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    onResize(width + (event.key === "ArrowLeft" ? -KEY_STEP : KEY_STEP));
    onDone();
  };

  return (
    <div
      className="pane-divider"
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={width}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      style={{ left: position }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onLostPointerCapture={endDrag}
      onDoubleClick={onReset}
      onKeyDown={onKeyDown}
    />
  );
}
