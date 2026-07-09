"use client";

import { useId } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useFocusTrap } from "@/hooks/useFocusTrap";
import { Button } from "@/components/ui/button";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  /** Body text explaining the consequence of confirming. */
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** Style the confirm action as destructive (red). Defaults to true. */
  destructive?: boolean;
}

/**
 * Modal confirmation for consequential actions. Matches the house overlay
 * pattern (backdrop blur + spring-in card, bottom sheet on mobile / centered on
 * desktop). Uses role="alertdialog" — the correct ARIA for a prompt that
 * interrupts to demand a decision — traps focus, and defaults focus to Cancel
 * so the safe choice is one Enter away. Escape and backdrop click both cancel.
 */
export function ConfirmDialog(props: ConfirmDialogProps) {
  return (
    <AnimatePresence>{props.open && <ConfirmOverlay {...props} />}</AnimatePresence>
  );
}

function ConfirmOverlay({
  title,
  description,
  confirmLabel,
  cancelLabel,
  onConfirm,
  onCancel,
  destructive = true,
}: ConfirmDialogProps) {
  const trapRef = useFocusTrap<HTMLDivElement>();
  const titleId = useId();
  const descId = useId();

  return (
    <div
      className="fixed inset-0 z-50 flex items-end sm:items-center justify-center overflow-hidden"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={descId}
      onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
      }}
    >
      {/* Backdrop — click to cancel (the safe default). */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.2 }}
        className="absolute inset-0 bg-background/80 backdrop-blur-md cursor-pointer"
        onClick={onCancel}
        role="presentation"
      />

      {/* Card */}
      <motion.div
        ref={trapRef}
        initial={{ y: 480, opacity: 0, scale: 0.98 }}
        animate={{ y: 0, opacity: 1, scale: 1 }}
        exit={{ y: 480, opacity: 0, scale: 0.98 }}
        transition={{ type: "spring", damping: 30, stiffness: 300 }}
        className="relative z-10 w-full max-w-none sm:max-w-[398px] mx-0 sm:mx-4 rounded-t-3xl sm:rounded-2xl border-t border-x sm:border border-border bg-card p-5 sm:p-6 shadow-2xl flex flex-col items-center gap-4 text-center"
      >
        {/* Warning glyph */}
        <div
          className={
            "flex size-12 items-center justify-center rounded-full " +
            (destructive
              ? "bg-destructive/10 text-destructive"
              : "bg-primary/10 text-primary")
          }
        >
          <WarningIcon size={24} />
        </div>

        <div className="flex flex-col gap-1.5">
          <h2 id={titleId} className="text-lg font-semibold tracking-tight">
            {title}
          </h2>
          <p id={descId} className="text-sm text-muted-foreground leading-relaxed text-balance">
            {description}
          </p>
        </div>

        {/* Actions: Cancel is the default focus + Enter target. */}
        <div className="mt-1 flex w-full flex-col-reverse gap-2 sm:flex-row">
          <Button
            variant="outline"
            size="lg"
            className="w-full sm:flex-1"
            autoFocus
            onClick={onCancel}
          >
            {cancelLabel}
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            size="lg"
            className="w-full sm:flex-1"
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </div>
      </motion.div>
    </div>
  );
}

function WarningIcon({ size = 24 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
    </svg>
  );
}
