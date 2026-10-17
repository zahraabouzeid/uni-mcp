export function Brand({ className = "" }: { className?: string }) {
  return (
    <span className={`inline-flex items-center tracking-tight ${className}`}>
      {/* The icon as a mask, so it takes the text colour. */}
      <span
        aria-hidden
        className="mr-[0.3em] size-[1.6em] bg-current [mask:url(/icon.svg)_center/contain_no-repeat]"
      />
      <span className="font-light">uni</span>
      <span className="font-bold">MCP</span>
    </span>
  );
}
