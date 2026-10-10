import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
} from "react";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary";
};

export function Button({ variant = "secondary", className = "", ...props }: ButtonProps) {
  const look =
    variant === "primary"
      ? "border-ink bg-ink text-canvas hover:opacity-85"
      : "border-line bg-canvas text-ink hover:bg-panel";
  return (
    <button
      type="button"
      className={`h-8 shrink-0 whitespace-nowrap rounded-md border px-3 text-[13px] font-medium disabled:opacity-40 ${look} ${className}`}
      {...props}
    />
  );
}

export function Table({ columns, children }: { columns?: string[]; children: ReactNode }) {
  return (
    <div className="overflow-x-auto rounded-md border border-line">
      <table className="w-full min-w-[32rem] border-collapse text-left">
        {columns && (
          <thead>
            <tr className="border-b border-line bg-panel text-xs text-muted">
              {columns.map((title, index) => (
                <th key={index} className="h-9 whitespace-nowrap px-4 font-medium">
                  {title}
                </th>
              ))}
            </tr>
          </thead>
        )}
        <tbody className="divide-y divide-line">{children}</tbody>
      </table>
    </div>
  );
}

export function Cell({ className = "", children }: { className?: string; children?: ReactNode }) {
  return <td className={`px-4 py-2.5 ${className}`}>{children}</td>;
}

export function ActionCell({ children }: { children: ReactNode }) {
  return <td className="w-px whitespace-nowrap px-4 py-2 text-right">{children}</td>;
}

export function DetailRow({ columns, children }: { columns: number; children: ReactNode }) {
  return (
    <tr>
      {/* max-w-0: wide content scrolls inside the cell instead of stretching the table */}
      <td colSpan={columns} className="max-w-0 bg-panel px-4 py-4">
        {children}
      </td>
    </tr>
  );
}

export function Status({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <span
      className={`inline-flex items-center gap-2 whitespace-nowrap ${ok ? "text-ink" : "text-muted"}`}
    >
      <span aria-hidden className={`size-2 rounded-full ${ok ? "bg-ok" : "bg-line"}`} />
      {children}
    </span>
  );
}

export function Field({
  label,
  ...input
}: { label: string } & InputHTMLAttributes<HTMLInputElement>) {
  return (
    <label className="grid w-60 gap-1">
      <span className="text-xs font-medium text-muted">{label}</span>
      <input
        spellCheck={false}
        className="h-8 rounded-md border border-line bg-canvas px-2.5 text-ink outline-none focus:border-ink"
        {...input}
      />
    </label>
  );
}

export function Choice({
  label,
  options,
  required,
  defaultValue,
  ...select
}: { label: string; options: string[] } & SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <label className="grid w-60 gap-1">
      <span className="text-xs font-medium text-muted">{label}</span>
      <select
        required={required}
        defaultValue={defaultValue}
        className="h-8 rounded-md border border-line bg-canvas px-2 text-ink outline-none focus:border-ink"
        {...select}
      >
        {!required && defaultValue === undefined && <option value="" />}
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </label>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="text-danger">
      {children}
    </p>
  );
}

export function CodeBlock({ code, className = "" }: { code: string; className?: string }) {
  return (
    <pre
      className={`overflow-auto rounded-md border border-line bg-canvas px-3 py-2.5 font-mono text-[13px] leading-5 ${className}`}
    >
      {code}
    </pre>
  );
}
