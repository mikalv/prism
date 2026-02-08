interface CitationBadgeProps {
  number: number
  onClick?: () => void
}

export function CitationBadge({ number, onClick }: CitationBadgeProps) {
  return (
    <button
      onClick={onClick}
      className="
        inline-flex items-center justify-center
        w-5 h-5
        rounded-full
        bg-[var(--accent)]/20
        text-[var(--accent)]
        text-xs font-medium
        hover:bg-[var(--accent)]/30
        transition-colors
      "
    >
      {number}
    </button>
  )
}
