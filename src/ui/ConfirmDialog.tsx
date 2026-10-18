import { useEffect, useRef } from "react";
import type { ConfirmationRequest } from "../ipc/agent";

interface Props {
  request: ConfirmationRequest;
  onAnswer: (approved: boolean) => void;
  /** Present when listening is set up; starts a spoken yes/no answer. */
  onAnswerBySpeaking?: () => void;
  listening: boolean;
  hint: string | null;
}

/**
 * Modal confirmation for a high-risk action. Shows exactly what will happen, using details
 * built by the backend from validated arguments. Focus starts on "No" (the safe choice);
 * Esc means No. Only the Yes button or a clear spoken "yes" approves.
 */
export function ConfirmDialog({ request, onAnswer, onAnswerBySpeaking, listening, hint }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const noButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
    noButton.current?.focus();
    return () => d?.close();
  }, [request.id]);

  return (
    <dialog
      ref={ref}
      className="confirm"
      aria-labelledby="confirm-title"
      aria-describedby="confirm-action confirm-details"
      onCancel={(e) => {
        e.preventDefault(); // Esc = No, through the same path as the button
        onAnswer(false);
      }}
    >
      {request.practice && (
        <p className="practice-badge">
          <span aria-hidden="true">🎓 </span>Practice: nothing will really happen
        </p>
      )}
      <h2 id="confirm-title">Please check before I go ahead</h2>
      <p id="confirm-action" className="confirm-action">
        {request.action}
      </p>
      {request.details.length > 0 && (
        <dl id="confirm-details" className="confirm-details">
          {request.details.map((d) => (
            <div key={d.label}>
              <dt>{d.label}</dt>
              <dd>{d.value}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="confirm-buttons">
        <button type="button" className="confirm-yes" onClick={() => onAnswer(true)}>
          <span aria-hidden="true">✓ </span>Yes, do it
        </button>
        <button type="button" className="confirm-no" ref={noButton} onClick={() => onAnswer(false)}>
          <span aria-hidden="true">✕ </span>No
        </button>
      </div>
      {onAnswerBySpeaking && (
        <button
          type="button"
          className="confirm-speak"
          aria-pressed={listening}
          disabled={listening}
          onClick={onAnswerBySpeaking}
        >
          <span aria-hidden="true">{listening ? "◉ " : "🎤 "}</span>
          {listening ? "Listening… say yes or no" : "Answer by speaking"}
        </button>
      )}
      <p className="hint-left">
        This question stays open for {Math.round(request.expiresInSecs / 60)} minutes. If you don't
        answer, I won't do it.
      </p>
      {hint && <p role="status">{hint}</p>}
    </dialog>
  );
}
