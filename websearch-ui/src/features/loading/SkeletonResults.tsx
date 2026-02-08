import { Skeleton } from '@/components/ui'

export function SkeletonResults() {
  return (
    <div className="grid grid-cols-3 gap-4 max-w-4xl mx-auto">
      {[1, 2, 3].map((i) => (
        <div key={i} className="p-4 rounded-[var(--radius-lg)] bg-[var(--bg-secondary)]">
          <Skeleton height={16} className="w-3/4 mb-3" />
          <Skeleton height={12} className="w-1/2 mb-2" />
          <Skeleton height={12} className="w-full mb-1" />
          <Skeleton height={12} className="w-5/6" />
        </div>
      ))}
    </div>
  )
}
