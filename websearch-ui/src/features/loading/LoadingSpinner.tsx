export function LoadingSpinner() {
  return (
    <div className="dot-spinner flex items-center gap-1.5">
      <span className="w-2 h-2 rounded-full bg-[var(--accent)]" />
      <span className="w-2 h-2 rounded-full bg-[var(--accent)]" />
      <span className="w-2 h-2 rounded-full bg-[var(--accent)]" />
    </div>
  )
}
